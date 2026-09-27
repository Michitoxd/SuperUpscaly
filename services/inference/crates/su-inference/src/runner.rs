//! Ejecucion de un pipeline sobre una imagen.
//!
//! El runner une las tres piezas: el motor de pipelines (`su-core`), la
//! planificacion de tiles (`su-tiling`) y el backend de inferencia. Su
//! responsabilidad es que **nunca se pierda un fallo por el camino**:
//!
//! - Cada etapa que no se ejecuta se reporta en `skipped` con su motivo.
//! - Cada degradacion (tile reducido, reintento) se cuenta y se devuelve.
//! - Un error de memoria no aborta el item: baja un escalon y reintenta.

use su_core::{EvalVars, Pipeline, StageId, SuError, SuResult, TileChoice};
use su_imageio::DecodedImage;
use su_tiling::{
    choose_tile, composite, degrade_tile, overlap_for_tile_boosted, TilePlan, VramBudget, MIN_TILE,
};

use crate::backend::{Backend, TileInput};

/// Configuracion de ejecucion. Se deriva de los ajustes del usuario y del
/// hardware detectado.
#[derive(Debug, Clone)]
pub struct RunnerConfig {
    /// Tile pedido por el usuario. `Auto` deja decidir al presupuesto de VRAM.
    pub tile_choice: TileChoice,
    /// Presupuesto de VRAM. `None` en CPU o cuando no se pudo consultar.
    pub budget: Option<VramBudget>,
    /// Candidatos de tile declarados por el modelo.
    pub candidates: Vec<u32>,
    /// Divisor del solape declarado por el modelo.
    pub overlap_divisor: u32,
    /// Alineacion de cada tile.
    pub pad_to: u32,
    /// Reintentos antes de darse por vencido con esta etapa.
    pub max_degradation_steps: u32,
    /// VRAM por megapixel de tile que declara el modelo del manifiesto.
    ///
    /// Manda sobre la calibracion del equipo: la calibracion mide lo que consume
    /// un modelo en **una** maquina concreta, y esto es lo que su autor dice que
    /// consume. Si existen las dos, la del modelo es la que vale.
    pub vram_per_megapixel: Option<f32>,
    /// Modelo elegido a mano que sustituye al de las etapas de escalado.
    ///
    /// Solo lo pone el modo Manual, y solo afecta a las etapas que escalan: las de
    /// restauracion conservan el suyo, porque no escalan y meterles un modelo que
    /// si lo hace cambiaria el tamano de la imagen.
    ///
    /// El usuario elige un modelo en la interfaz y espera que se use. Antes esta
    /// eleccion se validaba, se guardaba y no llegaba nunca al motor.
    pub model_override: Option<String>,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            tile_choice: TileChoice::Auto,
            budget: None,
            candidates: su_tiling::DEFAULT_CANDIDATES.to_vec(),
            overlap_divisor: 16,
            pad_to: 32,
            max_degradation_steps: 4,
            vram_per_megapixel: None,
            model_override: None,
        }
    }
}

/// Pistas de tiling que declara un modelo en el manifiesto.
///
/// Es el reflejo de `su_models::TilingHints` con lo que el runner necesita. Se
/// preguntan al proveedor ([`BackendProvider::tiling_hints`]) y no se copian en
/// [`RunnerConfig`] por lo mismo que `native_scale`: el proveedor es el unico que
/// sabe que modelo se va a usar de verdad —el del pipeline, el de reserva o el que
/// el usuario fijo a mano— y una copia en la configuracion habria que mantenerla
/// sincronizada en los tres sitios que construyen el runner (el CLI, el servidor y
/// la cola de trabajos) y en cada trabajo nuevo.
///
/// Hasta que existe esto, la seccion `tiling` del manifiesto se parseaba, se
/// validaba y se probaba... y no llegaba nunca al runner: todo se ejecutaba con los
/// valores por defecto. Un modelo que declara `vramPerMegapixel: 250` se ejecutaba
/// con 600 y recibia un tile mas pequeno del que necesita; uno que declara
/// `candidates: [768, 512]` porque el tile 1024 le hace producir artefactos seguia
/// recibiendo 1024.
#[derive(Debug, Clone, PartialEq)]
pub struct TilingHints {
    /// Tamanos de tile que el modelo admite.
    pub candidates: Vec<u32>,
    /// Divisor del solape: `tile / divisor`.
    pub overlap_divisor: u32,
    /// Alineacion de los tiles, en pixeles.
    pub pad_to: u32,
    /// VRAM por megapixel de tile, si el modelo la declara.
    pub vram_per_megapixel: Option<f32>,
}

impl RunnerConfig {
    /// Copia de esta configuracion con las pistas de un modelo aplicadas.
    ///
    /// Se devuelve una copia, y no se muta la configuracion compartida, porque cada
    /// etapa usa un modelo distinto: aplicar las pistas de la etapa que escala a la
    /// de restauracion seria usar el tile de otro modelo.
    pub fn with_hints(&self, hints: Option<&TilingHints>) -> RunnerConfig {
        let Some(hints) = hints else {
            return self.clone();
        };

        let mut config = self.clone();

        // Un manifiesto con candidatos por debajo del minimo se ignora en lugar de
        // dejar el planificador sin ningun tamano utilizable.
        let candidates: Vec<u32> = hints
            .candidates
            .iter()
            .copied()
            .filter(|candidate| *candidate >= su_tiling::MIN_TILE)
            .collect();
        if !candidates.is_empty() {
            config.candidates = candidates;
        }

        if hints.overlap_divisor > 0 {
            config.overlap_divisor = hints.overlap_divisor;
        }

        if hints.pad_to > 0 {
            config.pad_to = hints.pad_to;
        }

        if let Some(per_mp) = hints.vram_per_megapixel.filter(|value| *value > 0.0) {
            config.vram_per_megapixel = Some(per_mp);
        }

        config
    }
}

/// Progreso de una etapa, para reenviarlo a la interfaz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageProgress {
    pub stage: StageId,
    /// Tiles completados de la etapa actual.
    pub done: u32,
    pub total: u32,
}

/// Resultado de ejecutar un pipeline completo.
#[derive(Debug)]
pub struct RunOutcome {
    pub image: DecodedImage,
    /// Etapas que se ejecutaron de verdad.
    pub executed: Vec<StageId>,
    /// Todo lo que hizo que esta imagen no salga exactamente como el pipeline
    /// describe, con el motivo. Se muestran en el resumen final: "¿por que esta
    /// imagen salio distinta?" tiene que tener respuesta.
    ///
    /// Incluye dos cosas distintas y las dos importan: etapas que **no** se
    /// ejecutaron (su condicion no se cumplia, o no estaban implementadas) y etapas
    /// que si se ejecutaron pero **con otro modelo** del declarado
    /// (`fallbackModel`). En el segundo caso la etapa tambien aparece en
    /// `executed`, porque se hizo; lo que se anota aqui es con que se hizo.
    pub skipped: Vec<(StageId, String)>,
    /// Cuantas veces hubo que reducir el tile por falta de memoria.
    pub degradations: u32,
    /// Tile con el que termino la ultima etapa de escalado.
    pub final_tile: Option<u32>,
}

/// Fabrica de backends. Cada etapa con modelo recibe el suyo.
///
/// Se abstrae en lugar de pasar un unico backend porque un pipeline real usa
/// varios modelos distintos (denoise, escalado, restauracion facial) y cada uno
/// tiene su propia sesion de ONNX Runtime.
///
/// Se exige `Send + Sync` para que la cola de trabajos pueda compartirla entre
/// hilos sin envolverla en un mutex.
pub trait BackendProvider: Send + Sync {
    /// Nombre del proveedor, tal y como debe aparecer en el informe final.
    ///
    /// Vive aqui, y no en `Capabilities`, porque el informe tiene que decir el
    /// proveedor que **se uso**, no el que se recomendo. Sin la feature `onnx`
    /// el backend es el de referencia aunque la maquina tenga TensorRT, y
    /// anunciar TensorRT en el resultado seria mentir en el unico sitio donde el
    /// usuario puede comprobarlo.
    fn name(&self) -> &str {
        "referencia"
    }

    /// Si el backend consume VRAM.
    ///
    /// Determina `hardware.isCpu` y `hardware.freeVramMb` en `EvalVars`, que es
    /// lo que leen las condiciones de los pipelines. Por defecto `false`: el
    /// backend de referencia y los simulados corren en CPU y no tienen memoria
    /// de GPU que presupuestar.
    fn uses_vram(&self) -> bool {
        false
    }

    /// Suelta lo que el proveedor tenga cargado.
    ///
    /// Se llama entre imagenes cuando el trabajo pide `unloadBetweenImages`, para
    /// que el pico de memoria no acumule el modelo de cada etapa. Por defecto no
    /// hace nada: los proveedores sin sesiones persistentes no tienen que
    /// soltar nada.
    fn unload(&self) {}

    fn backend_for(&self, model_id: &str) -> SuResult<Box<dyn Backend>>;

    /// Escala nativa de un modelo **sin cargarlo**.
    ///
    /// Existe para poder decidir si una etapa escala antes de abrir una sesion de
    /// inferencia. Cargar un modelo de cientos de megabytes para leer un numero y
    /// tirarlo acto seguido es tiempo perdido en el mejor de los casos y una
    /// reserva de VRAM inutil en el peor.
    ///
    /// Devolver `None` es una respuesta legitima: "no lo se sin cargarlo". Quien
    /// pregunta debe entonces ser prudente, no inventarse una escala.
    fn native_scale(&self, _model_id: &str) -> Option<u32> {
        None
    }

    /// Pistas de tiling declaradas por el modelo en el manifiesto.
    ///
    /// Ver [`TilingHints`]: sin esto, la seccion `tiling` del manifiesto es codigo
    /// muerto y todos los modelos se ejecutan con el tile del primero que se penso.
    /// El valor por defecto es `None` porque un proveedor que no conoce el manifiesto
    /// —el clasico, el simulado, el de las pruebas— no tiene nada que declarar.
    fn tiling_hints(&self, model_id: &str) -> Option<TilingHints> {
        let _ = model_id;
        None
    }
}

/// Modelo que debe usar una etapa, aplicando la eleccion manual del usuario.
///
/// El modelo elegido a mano sustituye al de las etapas que **escalan**. A las de
/// restauracion no: no escalan, y meterles un modelo que si lo hace cambiaria el
/// tamano de la imagen sin que nadie lo haya pedido.
///
/// Se resuelve antes de cargar nada porque la decision depende solo de la escala,
/// y la escala de la etapa esta en el pipeline o la sabe el proveedor. Ver
/// [`BackendProvider::native_scale`].
fn model_for_stage<'a>(
    stage_model: &'a str,
    scale_out: Option<u32>,
    config: &'a RunnerConfig,
    provider: &dyn BackendProvider,
) -> &'a str {
    let Some(chosen) = config.model_override.as_deref() else {
        return stage_model;
    };
    if chosen == stage_model {
        return stage_model;
    }

    let scales = match scale_out {
        Some(value) => value > 1,
        // Sin `scaleOut` la escala la pone el modelo. Si el proveedor no puede
        // decirla sin cargarlo, no se sustituye: es preferible no aplicar el ajuste
        // a cambio de escalar con un modelo de restauracion y devolver un tamano
        // distinto del pedido.
        None => provider
            .native_scale(stage_model)
            .is_some_and(|scale| scale > 1),
    };

    if scales {
        chosen
    } else {
        stage_model
    }
}

/// Escala deducida del identificador del modelo.
///
/// Se busca un token `<digito>x` o `x<digito>` en el nombre — `4x-ultrasharp`
/// y `realesrgan-x4plus` dan 4, `2x-animesharpv3` da 2.
///
/// ## El caso por defecto es 1, y eso importa
///
/// Antes devolvia 4 para todo lo que no empezara por `2x` u `8x`, y eso hacia
/// que los modelos de restauracion (`scunet-color`, `gfpgan-v1.4`) pasaran por
/// escaladores de 4x. Como el runner toma la escala del backend cuando la
/// etapa no declara `scaleOut` — y las etapas de restauracion no lo declaran,
/// porque devuelven una imagen del mismo tamano que la de entrada — el
/// resultado era un pipeline multiplicado por 4 sin que nada lo avisara.
///
/// El caso por defecto correcto para un modelo sin marca de escala es 1: no
/// cambia el tamano.
///
/// Solo se admite un digito porque `Scale` solo tiene 2, 4 y 8.
pub fn scale_from_model_id(model_id: &str) -> u32 {
    let bytes = model_id.as_bytes();

    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'x' {
            continue;
        }

        let before = index.checked_sub(1).and_then(|i| bytes.get(i)).copied();
        let after = bytes.get(index + 1).copied();

        for candidate in [before, after].into_iter().flatten() {
            if candidate.is_ascii_digit() && candidate != b'0' {
                return u32::from(candidate - b'0');
            }
        }
    }

    1
}

/// Proveedor de backends simulados. Determina la escala por el identificador del
/// modelo, que es suficiente para los pipelines embebidos.
#[derive(Debug, Clone, Default)]
pub struct MockBackendProvider;

impl MockBackendProvider {
    /// Escala deducida del identificador del modelo. Ver [`scale_from_model_id`].
    pub fn scale_for(model_id: &str) -> u32 {
        scale_from_model_id(model_id)
    }
}

impl BackendProvider for MockBackendProvider {
    fn backend_for(&self, model_id: &str) -> SuResult<Box<dyn Backend>> {
        Ok(Box::new(
            crate::backend::MockBackend::new(Self::scale_for(model_id))?.with_id(model_id),
        ))
    }

    fn native_scale(&self, model_id: &str) -> Option<u32> {
        // El backend de referencia deduce la escala del identificador, asi que
        // puede responder sin construir nada.
        Some(Self::scale_for(model_id))
    }
}

/// Proveedor clasico: interpolacion de alta calidad en lugar de modelos.
///
/// Es el motor que se usa cuando no hay ONNX Runtime, o cuando no hay ningun
/// modelo instalado todavia. Ver [`crate::backend::ClassicalBackend`]: la
/// diferencia con `MockBackendProvider` no es cosmetica, es la que separa "bloques
/// de 4x4 con las zonas planas lavadas" de "un interpolador de verdad".
///
/// ## Lo que este motor no puede hacer, y por que se dice
///
/// Sobre un dibujo de lineas de 1 px, un interpolador reparte el escalon en una
/// rampa de unos 4 px de fuente. No es un defecto de la implementacion: es
/// interpolacion. Recuperar un borde duro a partir de una rampa es
/// deconvolucion, y eso es justo lo que aprende un modelo entrenado con dibujos.
/// Medido sobre el mismo caso: 6,3 px de transicion con el motor clasico frente a
/// 2,2-2,6 px con el modelo de anime (y 1,0 px con vecino mas cercano, que a
/// cambio deja escalones de 4 px en cada diagonal).
///
/// Por eso el nombre del motor acaba en el informe de cada imagen: el usuario
/// tiene que poder saber si el resultado lo hizo un modelo o un interpolador, y
/// decidir si le merece la pena descargar el modelo.
#[derive(Debug, Clone)]
pub struct ClassicalProvider {
    kernel: su_imageio::ResizeKernel,
    name: String,
}

impl ClassicalProvider {
    pub fn new(kernel: su_imageio::ResizeKernel) -> Self {
        Self {
            kernel,
            name: format!("clasico-{}", kernel.as_str()),
        }
    }
}

impl Default for ClassicalProvider {
    fn default() -> Self {
        // Catmull-Rom y no Lanczos3, medido sobre un dibujo de lineas de 128 px a
        // x4: los dos dejan la misma transicion (6,3 px frente a 6,2), pero
        // Catmull-Rom conserva mas superficie plana (87,8% frente a 83,1%), porque
        // su lóbulo negativo es menor. En arte de color plano, cada pixel que deja
        // de ser plano es un anillo alrededor de una linea.
        Self::new(su_imageio::ResizeKernel::CatmullRom)
    }
}

impl BackendProvider for ClassicalProvider {
    fn name(&self) -> &str {
        // El nombre acaba en el informe del trabajo y en la interfaz. Tiene que
        // permitir distinguir "lo hizo un modelo" de "lo hizo un interpolador":
        // son resultados distintos y el usuario tiene derecho a saber cual ve. Un
        // nombre generico como "referencia" no lo dice.
        &self.name
    }

    fn backend_for(&self, model_id: &str) -> SuResult<Box<dyn Backend>> {
        Ok(Box::new(
            crate::backend::ClassicalBackend::new(scale_from_model_id(model_id), self.kernel)?
                .with_id(model_id),
        ))
    }

    fn native_scale(&self, model_id: &str) -> Option<u32> {
        Some(scale_from_model_id(model_id))
    }
}

/// `true` si omitir esta etapa **no cambia el tamano del resultado**.
///
/// Es la condicion que decide si un modelo ausente permite continuar o tiene que
/// fallar la imagen. El criterio es el mismo que usa el runner para componer la
/// escala: lo que cuenta es `scaleOut`, no la escala nativa del modelo.
///
/// El caso que lo justifica es `lineclean`: usa un modelo x4 para limpiar lineas y
/// declara `scaleOut: 1`, asi que no aporta nada a la escala final. Antes, si ese
/// modelo no estaba instalado, la imagen fallaba con "modelo no disponible" aunque
/// el escalado de verdad (otra etapa, otro modelo) si estuviera listo.
fn stage_is_size_neutral(
    stage: &su_core::Stage,
    provider: &dyn BackendProvider,
    model_id: &str,
) -> bool {
    match stage.scale_out {
        Some(declared) => declared <= 1,
        // Sin `scaleOut`, la escala la pone el modelo. Si el proveedor no la sabe
        // sin cargarlo, la respuesta segura es `false`: es preferible fallar con un
        // mensaje claro a devolver una imagen con otro tamano del pedido.
        None => provider
            .native_scale(model_id)
            .is_some_and(|scale| scale <= 1),
    }
}

/// Filtro de reescalado declarado por una etapa.
///
/// El campo `"kernel"` de una etapa `resize` existia en el pipeline y **no se
/// leia**: todas las reducciones y ampliaciones se hacian con Lanczos3 pasara lo
/// que pasara, asi que un pipeline que pidiera `"kernel": "nearest"` para arte de
/// pixeles recibia otra cosa sin que nada lo dijera.
fn stage_kernel(stage: &su_core::Stage) -> SuResult<su_imageio::ResizeKernel> {
    match stage.kernel.as_deref() {
        Some(text) => su_imageio::ResizeKernel::parse(text),
        None => Ok(su_imageio::ResizeKernel::default()),
    }
}

// ---------------------------------------------------------------------------
// Restauracion facial
// ---------------------------------------------------------------------------

/// Lado del recorte que se le pasa al modelo de restauracion facial.
///
/// 512 es el tamano con el que se entrena y se exporta GFPGAN: las caras se
/// alinean, se llevan a 512x512 y se restauran a esa resolucion. Usar un tamano
/// fijo tiene ademas una ventaja practica: funciona igual con una exportacion de
/// entrada dinamica y con una de entrada fija, que rechazaria cualquier otra.
const FACE_CROP_SIDE: u32 = 512;

/// Margen alrededor de la caja de la cara, como fraccion de su lado mayor.
///
/// El modelo necesita contexto —pelo, orejas, cuello— para no inventarse el
/// contorno de la cara, y ese margen es tambien donde vive la transicion de la
/// mascara: sin el, la mezcla caeria sobre la propia cara.
const FACE_CROP_PADDING: f32 = 0.35;

/// Dos cajas cuyo solape supere esta fraccion del area menor describen la misma
/// cara. Pasa con detectores que devuelven varias cajas por rostro; procesarlas
/// por separado seria gastar dos inferencias para pegar una sobre otra.
const FACE_MERGE_OVERLAP: f32 = 0.5;

/// Por debajo de este recorte, el modelo no tiene informacion suficiente y
/// devuelve una cara inventada. Es mejor dejarla como estaba y decirlo.
const MIN_FACE_CROP_SIDE: u32 = 96;

/// Fusiona las cajas que describen la misma cara.
///
/// Se repite hasta que el resultado deja de cambiar: unir dos cajas puede hacer
/// que la caja nueva se solape con una tercera que antes no lo hacia, y con caras
/// muy juntas (un grupo, una foto de familia) eso ocurre de verdad.
pub fn merge_face_boxes(faces: &[su_core::FaceBox]) -> Vec<su_core::FaceBox> {
    let mut merged: Vec<su_core::FaceBox> = faces
        .iter()
        .copied()
        .filter(su_core::FaceBox::is_reliable)
        .collect();

    loop {
        let mut changed = false;
        let mut result: Vec<su_core::FaceBox> = Vec::with_capacity(merged.len());

        'cajas: for face in merged {
            for existing in result.iter_mut() {
                if overlap_ratio(existing, &face) > FACE_MERGE_OVERLAP {
                    *existing = union_box(existing, &face);
                    changed = true;
                    continue 'cajas;
                }
            }
            result.push(face);
        }

        merged = result;
        if !changed {
            return merged;
        }
    }
}

/// Solape entre dos cajas como fraccion del area de la menor.
fn overlap_ratio(a: &su_core::FaceBox, b: &su_core::FaceBox) -> f32 {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.w).min(b.x + b.w);
    let bottom = (a.y + a.h).min(b.y + b.h);

    if right <= left || bottom <= top {
        return 0.0;
    }

    let intersection = (right - left) * (bottom - top);
    let smallest = (a.w * a.h).min(b.w * b.h);
    if smallest <= 0.0 {
        return 0.0;
    }

    intersection / smallest
}

/// Caja que contiene a las dos, con la confianza mayor.
fn union_box(a: &su_core::FaceBox, b: &su_core::FaceBox) -> su_core::FaceBox {
    let left = a.x.min(b.x);
    let top = a.y.min(b.y);
    let right = (a.x + a.w).max(b.x + b.w);

    su_core::FaceBox {
        x: left,
        y: top,
        w: right - left,
        h: (a.y + a.h).max(b.y + b.h) - top,
        confidence: a.confidence.max(b.confidence),
    }
}

/// Recorte cuadrado centrado en una cara: `(izquierda, arriba, lado)`.
///
/// El cuadrado se **desplaza** para caber dentro de la imagen en lugar de
/// recortarse: una cara pegada al borde tiene que entrar entera, y el margen que
/// se pierde por un lado se compensa con el que sobra por el otro.
pub(crate) fn face_crop_rect(face: &su_core::FaceBox, width: u32, height: u32) -> (u32, u32, u32) {
    let box_width = (face.w * width as f32).max(1.0);
    let box_height = (face.h * height as f32).max(1.0);

    let wanted = box_width.max(box_height) * (1.0 + 2.0 * FACE_CROP_PADDING);
    let side = wanted.round().clamp(1.0, width.min(height) as f32) as u32;

    let center_x = (face.x + face.w / 2.0) * width as f32;
    let center_y = (face.y + face.h / 2.0) * height as f32;

    let left = (center_x - side as f32 / 2.0)
        .round()
        .clamp(0.0, (width - side) as f32) as u32;
    let top = (center_y - side as f32 / 2.0)
        .round()
        .clamp(0.0, (height - side) as f32) as u32;

    (left, top, side)
}

/// Resultado de la restauracion facial de una imagen.
pub(crate) struct FaceRestoreOutcome {
    pub(crate) image: DecodedImage,
    /// Caras efectivamente restauradas.
    pub(crate) processed: usize,
    /// Caras descartadas por ser demasiado pequenas para el modelo.
    pub(crate) too_small: usize,
}

/// Restaura las caras detectadas, una por una, y las pega con una mascara suave.
///
/// ## Por que por recortes y no de una vez
///
/// El modelo de restauracion facial es de escala 1: no amplia, arregla. Pasado por
/// la imagen entera, cada cara ocupa una fraccion minuscula de la entrada, asi que
/// el modelo la ve como un par de pixeles y lo unico que puede hacer es repartir
/// color. Recortando la cara **y dandole sus 512x512** el modelo ve una cara, que
/// es para lo que fue entrenado.
///
/// ## Por que con mascara
///
/// Un recorte pegado tal cual deja cuatro costuras rectas alrededor de la cara,
/// porque el color del recorte no coincide exactamente con el de la imagen de
/// fuera. La mascara radial vale 1 en el centro (la cara) y cae a 0 en el borde del
/// recorte, asi que la union es un degradado y no un corte. El peso final es la
/// intensidad que pidio el usuario (`prefs.faceRestore`), multiplicada por el
/// `blend` que declare la etapa: la preferencia manda, y un pipeline puede acotar.
pub(crate) fn restore_faces(
    source: &DecodedImage,
    faces: &[su_core::FaceBox],
    weight: f32,
    backend: &mut dyn Backend,
    stage: StageId,
    on_progress: &mut dyn FnMut(StageProgress),
) -> SuResult<FaceRestoreOutcome> {
    // Cada cara con su recorte. Se descartan las que no dan para el modelo, y se
    // cuentan aparte: omitirlas en silencio haria que el usuario no supiera por que
    // en una foto de grupo solo se han restaurado las caras grandes.
    let mut planned: Vec<(su_core::FaceBox, (u32, u32, u32))> = merge_face_boxes(faces)
        .into_iter()
        .map(|face| {
            let rect = face_crop_rect(&face, source.width, source.height);
            (face, rect)
        })
        .collect();

    let total_boxes = planned.len();
    planned.retain(|(_, (_, _, side))| *side >= MIN_FACE_CROP_SIDE);
    let too_small = total_boxes - planned.len();

    let mut image = source.clone();
    let total = planned.len() as u32;

    for (index, (face, (left, top, side))) in planned.iter().enumerate() {
        let (left, top, side) = (*left, *top, *side);
        let crop = su_imageio::crop(source, left, top, side, side)?;

        // El recorte se lleva al tamano que espera el modelo. Se hace con Lanczos
        // porque aqui no se esta escalando el resultado: esto es transporte, y el
        // modelo vuelve a poner el detalle.
        let prepared = su_imageio::resize(
            &crop,
            FACE_CROP_SIDE,
            FACE_CROP_SIDE,
            su_imageio::ResizeKernel::Lanczos3,
        )?;

        let input = TileInput::new(prepared.width, prepared.height, 3, &prepared.rgb)?;
        let restored = backend.run_tile(&input)?;

        if restored.channels != 3 {
            return Err(SuError::Internal(format!(
                "el modelo de restauracion facial '{}' devolvio {} canales",
                backend.id(),
                restored.channels
            )));
        }

        // Un modelo de restauracion puede devolver el recorte a otra escala (hay
        // exportaciones a x2 y a x4). Se vuelve al tamano del recorte, y si ya
        // cuadra no se toca un solo pixel: un reescalado de ida y vuelta solo puede
        // perder detalle.
        let patch = if restored.width == side && restored.height == side {
            DecodedImage {
                width: side,
                height: side,
                rgb: restored.data,
                alpha: None,
                icc_profile: crop.icc_profile.clone(),
                applied_orientation: crop.applied_orientation,
            }
        } else {
            let output = DecodedImage {
                width: restored.width,
                height: restored.height,
                rgb: restored.data,
                alpha: None,
                icc_profile: crop.icc_profile.clone(),
                applied_orientation: crop.applied_orientation,
            };
            su_imageio::resize(&output, side, side, su_imageio::ResizeKernel::Lanczos3)?
        };

        // Radio a plena intensidad: el que cubre la caja de la cara entera, con su
        // esquina. Asi el rostro se restaura por completo y la transicion queda
        // fuera de el, en el margen.
        let box_width = (face.w * source.width as f32).max(1.0);
        let box_height = (face.h * source.height as f32).max(1.0);
        let inner = (box_width.hypot(box_height) / side as f32).clamp(0.0, 0.95);
        let mask = su_imageio::radial_mask(side, side, inner);

        su_imageio::paste_masked(&mut image, &patch, left, top, &mask, weight)?;

        on_progress(StageProgress {
            stage,
            done: (index + 1) as u32,
            total,
        });
    }

    Ok(FaceRestoreOutcome {
        image,
        processed: planned.len(),
        too_small,
    })
}

/// Envuelve un canal alfa suelto en una imagen para poder escalarlo con la misma
/// maquinaria que el color. El canal se replica en los tres componentes porque el
/// modelo espera tres canales; despues se toma solo el primero.
fn alpha_channel_image(alpha: &[f32], width: u32, height: u32) -> DecodedImage {
    let mut rgb = Vec::with_capacity(alpha.len() * 3);
    for value in alpha {
        rgb.push(*value);
        rgb.push(*value);
        rgb.push(*value);
    }
    DecodedImage {
        width,
        height,
        rgb,
        alpha: None,
        icc_profile: None,
        applied_orientation: 1,
    }
}

/// Ejecuta el pipeline completo.
pub fn run_pipeline(
    source: &DecodedImage,
    pipeline: &Pipeline,
    vars: &EvalVars,
    provider: &dyn BackendProvider,
    config: &RunnerConfig,
    on_progress: &mut dyn FnMut(StageProgress),
) -> SuResult<RunOutcome> {
    // La imagen que va saliendo de las etapas, o `None` mientras siga siendo la de
    // entrada. Se guarda asi, y no como `source.clone()`, porque la mayoria de los
    // pipelines empiezan con una etapa que ya construye una imagen nueva: el clon
    // inicial era una copia entera del RGB —240 MB en una foto de 20 MP— que se
    // tiraba sin llegar a leerse. Con la primera etapa de por medio, la entrada se
    // lee prestada y no se copia nunca.
    let mut current: Option<DecodedImage> = None;
    let mut executed = Vec::new();
    let mut skipped = Vec::new();
    let mut degradations = 0;
    let mut final_tile = None;

    // La silueta de la imagen —su canal alfa— recorre **la misma cadena que el
    // color**, etapa por etapa y por la misma malla de tiles, en lugar de
    // reescalarse aparte.
    //
    // Antes se interpolaba con Lanczos al final, y el resultado era un borde de
    // dos velocidades: el modelo reconstruia el escalon del color en 1 px mientras
    // la silueta se quedaba en una rampa de 4 a 7 px con anillo. Medido sobre un
    // dibujo con transparencia, el borde superior pasaba de `0,0,0,70,255` en el
    // original a `0,1,2,1,0,0,0,0,0,5,19,45,85,138,194,239,255`: la silueta
    // difuminada se ve como un halo alrededor de todo el contorno, y es justo lo
    // que hace que una imagen escalada parezca blanda aunque el interior este
    // nitido. El mismo modelo sobre el canal alfa deja la rampa en 1 px —igual que
    // la referencia— y conserva los degradados suaves que si deben ser suaves
    // (medido: salto maximo entre pixeles vecinos de 2/255 en un alfa radial).
    //
    // El coste es una pasada mas de inferencia **solo en imagenes con
    // transparencia**: una foto sin canal alfa no paga nada.
    let mut silhouette = source
        .alpha
        .as_ref()
        .map(|alpha| alpha_channel_image(alpha, source.width, source.height));

    // Una etapa que escala se ejecuta dos veces cuando hay silueta. El progreso se
    // reparte entre las dos pasadas para que la barra avance una sola vez: sin
    // esto daria dos vueltas de 0 a 100% y pareceria que la etapa se ha reiniciado
    // a mitad. Se calcula por etapa, porque las de restauracion no llevan silueta.

    // AC-04: la escala prometida es un contrato, no una sugerencia. Si el
    // resultado no cabe en el limite configurado se rechaza **antes** de empezar,
    // en lugar de omitir una etapa de escalado a mitad de la cadena: eso devuelve
    // otro tamano, y el usuario acaba con un archivo que no es el que pidio sin
    // que nada se lo diga. Antes esto era exactamente lo que pasaba con `upscale2`
    // en los pipelines de 8x.
    if vars.max_output_mp > 0.0 && vars.estimated_output_mp > vars.max_output_mp {
        return Err(SuError::OutputTooLarge {
            scale: pipeline.scale.factor(),
            estimated_mp: vars.estimated_output_mp,
            limit_mp: vars.max_output_mp,
        });
    }

    for stage in &pipeline.stages {
        let reported = stage.reported_stage();

        if !stage.is_active(vars)? {
            // Una condicion que no se cumple no es un fallo, pero **cambia el
            // resultado** y por eso tiene que quedar dicha. El caso que lo hace
            // evidente es `upscale2` en los pipelines de 8x: si no entra, la
            // imagen sale a 2x aunque se pidieran 8x. Antes esto era un `continue`
            // mudo, asi que el resumen no podia responder a "¿por que esta imagen
            // salio distinta?", que es justo lo que promete `RunOutcome::skipped`.
            skipped.push((
                reported,
                format!("la condicion de la etapa '{}' no se cumple", stage.id),
            ));
            continue;
        }

        // Lo que ven las etapas: la salida de la anterior, o la imagen de entrada
        // si todavia no ha corrido ninguna.
        let input: &DecodedImage = current.as_ref().unwrap_or(source);

        match stage.op {
            su_core::StageOp::Analyze => {
                // El analisis se hace antes de entrar aqui; el resultado llega en
                // `vars`. La etapa existe en el pipeline para dejar constancia de
                // que se ejecuto.
                executed.push(reported);
            }

            su_core::StageOp::Model => {
                let Some(stage_model) = stage.model.as_deref() else {
                    skipped.push((reported, "la etapa no declara modelo".to_string()));
                    continue;
                };

                let requested = model_for_stage(stage_model, stage.scale_out, config, provider);

                // Tres desenlaces posibles, en este orden: el modelo pedido, su
                // equivalente de reserva y, si la etapa no cambia el tamano, la
                // omision con su motivo. Un fallo solo donde de verdad no hay
                // salida: cuando la etapa es la que escala.
                let mut model_id = requested;
                let mut backend: Box<dyn Backend> = match provider.backend_for(requested) {
                    Ok(backend) => backend,
                    Err(error) => {
                        // `fallbackModel`: cada etapa con modelo declara un
                        // equivalente funcional para cuando el preferido no esta.
                        // El campo existia en los seis pipelines embebidos, estaba
                        // documentado y **no se leia**: si el usuario descargaba el
                        // modelo de reserva y no el principal, la imagen fallaba con
                        // "modelo no instalado" teniendo uno que sirve al lado.
                        let fallback = stage
                            .fallback_model
                            .as_deref()
                            .filter(|candidate| *candidate != requested)
                            .and_then(|candidate| {
                                provider.backend_for(candidate).ok().map(|backend| (candidate, backend))
                            });

                        match fallback {
                            Some((candidate, substitute)) => {
                                skipped.push((
                                    reported,
                                    format!(
                                        "modelo de reserva: se uso '{candidate}' porque '{requested}' no se pudo cargar ({error})"
                                    ),
                                ));
                                model_id = candidate;
                                substitute
                            }
                            None => {
                                // Sin sustituto. Si la etapa no aporta escala, se
                                // omite y se dice: el resto de la cadena —y en
                                // particular el escalado— sigue su camino. Si aporta
                                // escala, se falla con el motivo, porque devolver
                                // otra medida seria peor que no devolver nada.
                                if stage_is_size_neutral(stage, provider, requested) {
                                    skipped.push((
                                        reported,
                                        format!(
                                            "etapa omitida: su modelo '{requested}' no esta disponible ({error}) y la etapa no cambia el tamano"
                                        ),
                                    ));
                                    continue;
                                }

                                return Err(error);
                            }
                        }
                    }
                };
                let native = backend.scale();
                // Si la etapa no declara escala, se asume la del modelo.
                let declared = stage.scale_out.unwrap_or(native);

                // `native` ya es la del modelo elegido, que puede ser el de la etapa o
                // el que el usuario fijo a mano. Antes se cargaba primero el del
                // pipeline y se sustituia despues, de modo que el proveedor abria una
                // sesion —y reservaba su VRAM— para no usarla.

                if native < declared {
                    return Err(SuError::Internal(format!(
                        "el modelo '{model_id}' escala x{native} pero la etapa '{}' pide x{declared}",
                        stage.id
                    )));
                }

                // `onlyOnFaces` dice literalmente lo que significa: la etapa se
                // aplica **sobre las caras**, no sobre la imagen que las contiene.
                //
                // Hasta ahora el campo se parseaba, se validaba y se ignoraba: la
                // imagen entera pasaba por GFPGAN y la restauracion facial que
                // anuncia la documentacion no ocurria. En un recorte de 512 px el
                // modelo ve una cara; en una foto de 20 MP ve un par de pixeles por
                // cara y reparte color.
                if stage.only_on_faces {
                    if !stage_is_size_neutral(stage, provider, model_id) {
                        return Err(SuError::Internal(format!(
                            "la etapa '{}' se declara sobre caras pero su modelo '{model_id}' escala x{native}",
                            stage.id
                        )));
                    }

                    // Sin caras no hay nada que restaurar, y se dice con esas
                    // palabras: es lo que el usuario ve cuando arrastra un paisaje
                    // con el modo Foto. Los pipelines embebidos ya lo evitan con su
                    // condicion, pero una cadena propia puede declarar `onlyOnFaces`
                    // sin ella.
                    if vars.faces.is_empty() {
                        skipped.push((reported, "no se detectaron caras".to_string()));
                        continue;
                    }

                    // El peso es la intensidad que pidio el usuario, acotada por el
                    // `blend` que declara la etapa. La preferencia manda porque es lo
                    // que el usuario acaba de elegir en la interfaz, y el `blend`
                    // sigue sirviendo para que un pipeline pueda decir "como maximo
                    // esto".
                    let weight =
                        vars.face_restore.intensity() * stage.blend_weight(vars)?.unwrap_or(1.0);

                    let outcome = restore_faces(
                        input,
                        &vars.faces,
                        weight,
                        backend.as_mut(),
                        reported,
                        on_progress,
                    )?;

                    if outcome.processed == 0 {
                        skipped.push((
                            reported,
                            format!(
                                "no hay caras que restaurar: {} caja(s) por debajo de {MIN_FACE_CROP_SIDE} px",
                                outcome.too_small
                            ),
                        ));
                        continue;
                    }

                    if outcome.too_small > 0 {
                        skipped.push((
                            reported,
                            format!(
                                "{} cara(s) demasiado pequenas para restaurar (menos de {MIN_FACE_CROP_SIDE} px de recorte)",
                                outcome.too_small
                            ),
                        ));
                    }

                    current = Some(outcome.image);
                    executed.push(reported);
                    continue;
                }

                // La silueta sigue solo las etapas que **cambian la geometria**. Una
                // etapa de restauracion usa un modelo que escala para limpiar y
                // despues devuelve la imagen a su tamano, asi que no aporta nada a la
                // forma: hacerla pasar por ella costaria una inferencia mas y, peor,
                // un reescalado de vuelta que volveria a difuminar el contorno que
                // acabamos de reconstruir. Medido en el pipeline de dibujo, la etapa
                // `lineclean` metia al alfa en un modelo x4 y lo reducia otra vez.
                // Las pistas del modelo que se va a usar de verdad, ya sea el del
                // pipeline, el de reserva o el que el usuario fijo a mano: son las
                // que deciden el tile, el solape y la alineacion de **esta** etapa.
                let stage_config = config.with_hints(provider.tiling_hints(model_id).as_ref());

                let silhouette_follows = !stage_is_size_neutral(stage, provider, model_id);
                let passes: u32 = if silhouette.is_some() && silhouette_follows {
                    2
                } else {
                    1
                };

                let produced = if native > 1 {
                    let mut color_tiles = 0u32;
                    let result = {
                        let mut report = |progress: StageProgress| {
                            color_tiles = progress.total;
                            on_progress(StageProgress {
                                stage: progress.stage,
                                done: progress.done,
                                total: progress.total * passes,
                            });
                        };
                        upscale_tiled(
                            input,
                            native,
                            stage.overlap_boost,
                            reported,
                            backend.as_mut(),
                            &stage_config,
                            &mut report,
                        )?
                    };
                    degradations += result.degradations;
                    final_tile = Some(result.tile);

                    // La segunda mitad de la etapa: la silueta, con el mismo modelo
                    // y el mismo tile. Se escala con el motor que haya —modelo o
                    // interpolador— para que la silueta y el color salgan siempre
                    // del mismo sitio.
                    let shape = if silhouette_follows {
                        silhouette.take()
                    } else {
                        None
                    };
                    if let Some(shape) = shape {
                        let mut report = |progress: StageProgress| {
                            on_progress(StageProgress {
                                stage: progress.stage,
                                done: color_tiles + progress.done,
                                total: color_tiles * passes,
                            });
                        };
                        let scaled = upscale_tiled(
                            &shape,
                            native,
                            stage.overlap_boost,
                            reported,
                            backend.as_mut(),
                            &stage_config,
                            &mut report,
                        )?;
                        degradations += scaled.degradations;
                        silhouette = Some(scaled.image);
                    }

                    result.image
                } else if backend.supports_full_image() {
                    let image = full_image_pass(input, backend.as_mut(), &stage_config)?;
                    on_progress(StageProgress {
                        stage: reported,
                        done: 1,
                        total: 1,
                    });
                    image
                } else {
                    skipped.push((reported, format!("'{model_id}' no admite imagen completa")));
                    continue;
                };

                // Un modelo x4 puede tener que devolver la imagen a su tamano
                // original: es el caso de las etapas de restauracion, que usan un
                // modelo que escala para limpiar, no para ampliar. Se reduce la
                // imagen **ya compuesta**, no cada tile: reducir tile a tile
                // desalinearia las muestras y dejaria costuras justo donde el
                // solape existe para evitarlas.
                let produced = if native > declared {
                    su_imageio::resize(
                        &produced,
                        scaled_dimension(input.width, declared),
                        scaled_dimension(input.height, declared),
                        stage_kernel(stage)?,
                    )?
                } else {
                    produced
                };

                // La reduccion tambien es geometria, asi que la silueta la sigue con
                // el mismo filtro y el mismo destino.
                silhouette = match silhouette {
                    Some(shape) if native > declared && silhouette_follows => {
                        Some(su_imageio::resize(
                            &shape,
                            scaled_dimension(input.width, declared),
                            scaled_dimension(input.height, declared),
                            stage_kernel(stage)?,
                        )?)
                    }
                    other => other,
                };

                // `blend` no se aplica al alfa: mezclar una salida de restauracion
                // sobre el color suaviza el color, pero la silueta no es un color que
                // se mezcle, es la forma de la imagen.
                let salida = match stage.blend_weight(vars)? {
                    Some(weight) => blend(input, &produced, weight)?,
                    None => produced,
                };
                current = Some(salida);

                executed.push(reported);
            }

            su_core::StageOp::Resize => {
                let factor = stage.factor.unwrap_or(1.0);
                // Un factor que no sea positivo es un error de autoria, y NaN
                // tambien: se escribe asi, y no como `!(factor > 0.0)`, porque la
                // negacion de una comparacion sobre un tipo parcialmente ordenado
                // no dice a quien lee que se contempla el NaN.
                if factor.is_nan() || factor <= 0.0 {
                    return Err(SuError::Internal(format!(
                        "factor de reescalado invalido en la etapa '{}'",
                        stage.id
                    )));
                }
                let width = ((input.width as f32) * factor).round().max(1.0) as u32;
                let height = ((input.height as f32) * factor).round().max(1.0) as u32;
                let redimensionada = su_imageio::resize(input, width, height, stage_kernel(stage)?)?;
                current = Some(redimensionada);
                // Una etapa de reescalado explicito cambia la geometria, y la
                // silueta tiene que seguirla o el canal alfa acabaria con otro
                // tamano que la imagen que lo lleva.
                if let Some(shape) = silhouette.take() {
                    if shape.width != width || shape.height != height {
                        silhouette = Some(su_imageio::resize(
                            &shape,
                            width,
                            height,
                            stage_kernel(stage)?,
                        )?);
                    } else {
                        silhouette = Some(shape);
                    }
                }
                executed.push(reported);
            }

            su_core::StageOp::Unsharp => {
                let enfocada = unsharp(
                    input,
                    stage.amount.unwrap_or(0.3),
                    stage.radius.unwrap_or(1.0),
                    stage.threshold.unwrap_or(0.0),
                )?;
                current = Some(enfocada);
                executed.push(reported);
            }

            su_core::StageOp::DenoiseClassic => {
                // La reduccion de ruido clasica llega en la Fase 3. Se reporta como
                // omitida en lugar de fingir que se hizo.
                skipped.push((reported, "denoise clasico: pendiente (Fase 3)".to_string()));
            }

            su_core::StageOp::Compose => {
                skipped.push((reported, "composicion de ramas: pendiente (Fase 3)".to_string()));
            }
        }
    }

    // AC-04, comprobado al final y no solo al principio: el pipeline promete una
    // escala en su identificador, y entregar otra cosa es un fallo aunque el
    // proceso haya terminado sin errores. La comprobacion previa cubre el limite
    // de megapixeles; esta cubre los pipelines mal escritos (un factor de mas, una
    // etapa de escalado condicionada). Si no cuadra, el motivo son las etapas que
    // se omitieron, asi que se incluyen: un error que no dice por que obliga a
    // reproducir el caso a mano.
    let expected_width = scaled_dimension(source.width, pipeline.scale.factor());
    let expected_height = scaled_dimension(source.height, pipeline.scale.factor());

    // Un pipeline sin ninguna etapa que sustituya la imagen no es un caso real, pero
    // si ocurre lo honesto es devolver la entrada (y con ella su alfa), no inventarse
    // una imagen vacia.
    let mut image = current.unwrap_or_else(|| source.clone());

    if image.width != expected_width || image.height != expected_height {
        let reason = if skipped.is_empty() {
            "no se omitio ninguna etapa: los factores del pipeline no dan esa escala".to_string()
        } else {
            skipped
                .iter()
                .map(|(stage, why)| format!("{}: {why}", stage.as_str()))
                .collect::<Vec<_>>()
                .join("; ")
        };

        return Err(SuError::ScaleNotReached {
            expected_scale: pipeline.scale.factor(),
            expected_width,
            expected_height,
            actual_width: image.width,
            actual_height: image.height,
            reason,
        });
    }

    // La silueta vuelve a la imagen como canal alfa. Se comprueba el tamano antes
    // de devolverla: un alfa de otra medida no es una imagen algo peor, es una
    // imagen corrupta, y el codificador la rechazaria mas tarde y peor.
    if let Some(shape) = silhouette {
        if shape.width != image.width || shape.height != image.height {
            return Err(SuError::Internal(format!(
                "la silueta salio a {}x{} y la imagen a {}x{}",
                shape.width, shape.height, image.width, image.height
            )));
        }
        image.alpha = Some(
            shape
                .rgb
                .chunks_exact(3)
                .map(|pixel| pixel[0].clamp(0.0, 1.0))
                .collect(),
        );
    }

    Ok(RunOutcome {
        image,
        executed,
        skipped,
        degradations,
        final_tile,
    })
}

/// Dimension de una etapa de escala: `round(actual * factor)`, nunca menor que 1.
fn scaled_dimension(actual: u32, factor: u32) -> u32 {
    (((actual as f64) * (factor as f64)).round() as u32).max(1)
}

/// Mezcla dos imagenes del mismo tamano.
///
/// `weight` es el peso del resultado de la etapa, no el de la base: un `blend` de
/// 0.7 en una etapa de restauracion significa "70% del modelo". Sin `blend`, el
/// resultado es el del modelo al 100%, que es el comportamiento de las etapas de
/// escalado.
///
/// Mezclar es lo que hace util a una pasada de restauracion. Sustituir los
/// pixeles enteros cambia el caracter de la imagen, y en un modelo x4 aplicado a
/// escala 1 el resultado sale mas suave de lo que el usuario pidio; el `blend` es
/// la perilla que evita tener que elegir entre "con artefactos" y "sin detalle".
///
/// Mezclar imagenes de distinto tamano no tiene sentido, asi que es un error y no
/// un ajuste silencioso: significa que el pipeline tiene un `blend` en una etapa
/// que ademas cambia la escala.
fn blend(base: &DecodedImage, over: &DecodedImage, weight: f32) -> SuResult<DecodedImage> {
    if base.width != over.width || base.height != over.height {
        return Err(SuError::Internal(format!(
            "no se puede mezclar una imagen {}x{} con otra {}x{}: la etapa cambia el tamano",
            base.width, base.height, over.width, over.height
        )));
    }

    let weight = weight.clamp(0.0, 1.0);
    let rgb = base
        .rgb
        .iter()
        .zip(over.rgb.iter())
        // `under + (top - under) * w` en lugar de `(1 - w) * under + w * top`: la
        // segunda forma pierde precision cuando `w` es pequeno y el resultado no
        // da exactamente la base al restar, que es lo que un test de identidad
        // espera.
        .map(|(under, top)| (under + (top - under) * weight).clamp(0.0, 1.0))
        .collect();

    Ok(DecodedImage {
        rgb,
        ..base.clone()
    })
}

struct TiledResult {
    image: DecodedImage,
    degradations: u32,
    tile: u32,
}

/// Escala una imagen por tiles, con degradacion progresiva ante falta de memoria.
///
/// `stage` es la etapa que se esta ejecutando, para que el progreso se reporte
/// bajo su nombre. No siempre es `Upscale`: una pasada de limpieza de lineas usa
/// un modelo x4 y se reporta como `Denoise`, y sin este parametro la barra de
/// progreso aparecia bajo la etapa equivocada.
#[allow(clippy::too_many_arguments)]
fn upscale_tiled(
    source: &DecodedImage,
    scale: u32,
    overlap_boost: u32,
    stage: StageId,
    backend: &mut dyn Backend,
    config: &RunnerConfig,
    on_progress: &mut dyn FnMut(StageProgress),
) -> SuResult<TiledResult> {
    let mut tile = initial_tile(source, config, backend);

    // El canvas se rellena con reflexion, no con negro: el relleno negro genera
    // halos en los bordes, un defecto clasico de otras implementaciones.
    let canvas = padded_canvas(source, config.pad_to)?;

    let mut degradations = 0u32;
    let mut last_error: Option<SuError> = None;

    for attempt in 0..=config.max_degradation_steps {
        let overlap = overlap_for_tile_boosted(tile, config.overlap_divisor, overlap_boost);
        let plan = TilePlan::new(
            source.width,
            source.height,
            tile,
            overlap,
            config.pad_to,
        )?;

        let total_tiles = plan.count();
        let mut done = 0u32;

        let result = composite(&plan, scale, 3, |current_tile| {
            let data = extract_tile(&canvas, &plan, current_tile);
            let input = TileInput::new(
                current_tile.read_w,
                current_tile.read_h,
                3,
                &data,
            )?;
            let output = backend.run_tile(&input)?;
            done += 1;
            on_progress(StageProgress {
                stage,
                done,
                total: total_tiles,
            });
            Ok(output.data)
        });

        match result {
            Ok(data) => {
                let image = DecodedImage {
                    width: source.width * scale,
                    height: source.height * scale,
                    rgb: data,
                    alpha: None,
                    icc_profile: source.icc_profile.clone(),
                    applied_orientation: source.applied_orientation,
                };
                return Ok(TiledResult {
                    image,
                    degradations,
                    tile,
                });
            }
            Err(error) if is_memory_error(&error) => {
                let Some(smaller) = degrade_tile(tile, &config.candidates, MIN_TILE) else {
                    return Err(error);
                };
                tracing::warn!(
                    from = tile,
                    to = smaller,
                    attempt,
                    "falta de memoria: se reduce el tile y se reintenta"
                );
                tile = smaller;
                degradations += 1;
                last_error = Some(error);
            }
            Err(other) => return Err(other),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        SuError::OutOfVram {
            tile,
            free_mb: config.budget.map(|budget| budget.free_mb).unwrap_or(0),
            needed_mb: 0,
        }
    }))
}

/// Un fallo que se puede resolver con un tile mas pequeno.
fn is_memory_error(error: &SuError) -> bool {
    matches!(error, SuError::OutOfVram { .. } | SuError::TileFailed { .. })
}

/// Tile inicial: el pedido por el usuario, o el mayor que quepa en el presupuesto.
fn initial_tile(source: &DecodedImage, config: &RunnerConfig, backend: &dyn Backend) -> u32 {
    if let Some(explicit) = config.tile_choice.explicit() {
        return explicit;
    }

    let mut budget = config.budget;
    if let Some(value) = budget.as_mut() {
        // Orden de precedencia, de mas a menos fiable: lo que declara el modelo en
        // el manifiesto, lo que declara el backend, y lo que ya traia el presupuesto
        // (que es la calibracion del equipo o el valor por defecto).
        if let Some(per_mp) = config
            .vram_per_megapixel
            .or_else(|| backend.vram_per_megapixel())
        {
            value.vram_per_megapixel = per_mp;
        }
    }

    let candidates = if config.candidates.is_empty() {
        su_tiling::DEFAULT_CANDIDATES
    } else {
        &config.candidates
    };

    match budget {
        Some(budget) => choose_tile(&budget, candidates).unwrap_or(MIN_TILE),

        // Sin datos de VRAM (CPU, o la consulta fallo) el tiling no va de memoria
        // sino del coste fijo de cada pasada.
        None => {
            // Se compara contra el lienzo ya alineado, no contra la imagen cruda:
            // el plan rellena hasta un multiplo de `pad_to` y decide `single_pass`
            // con esa medida, asi que pedir un tile de 512 para una imagen de 500 px
            // que se alinea a 512 daria un unico tile... y pedirlo de 500 no.
            let pad_to = config.pad_to.max(1);
            let largest_side = source.width.max(source.height).div_ceil(pad_to) * pad_to;
            choose_tile_without_budget(largest_side, candidates)
        }
    }
}

/// Tile inicial cuando no hay presupuesto de VRAM (CPU, o la consulta fallo).
///
/// El criterio es el coste fijo de cada pasada, no la memoria:
///
/// - Si la imagen cabe en un tile, **el menor de los que la cubren**. Trocear una
///   imagen pequena solo anade costuras y trabajo de composicion.
/// - Si no cabe, **el mayor de los candidatos**. Cada tile paga una inferencia
///   completa y un solape; con el tile mas pequeno, una imagen de 3000x2000 pasa de
///   ~12 tiles a ~180, y el solape se come una parte proporcional de cada uno.
///
/// La segunda rama estaba escrita al reves: cogia el candidato mas pequeno que
/// pasara de 256, asi que el caso mas costoso —una imagen grande en CPU— era
/// justo el que peor tile recibia. Medido con el backend simulado sobre 1500x1500:
/// 64 tiles de 256 px frente a 4 de 1024.
pub fn choose_tile_without_budget(largest_side: u32, candidates: &[u32]) -> u32 {
    let mut validos: Vec<u32> = candidates
        .iter()
        .copied()
        .filter(|candidate| *candidate >= MIN_TILE)
        .collect();

    if validos.is_empty() {
        return MIN_TILE;
    }

    validos.sort_unstable();

    validos
        .iter()
        .copied()
        .find(|candidate| *candidate >= largest_side)
        .unwrap_or_else(|| validos[validos.len() - 1])
}

/// Rellena la imagen hasta un multiplo de `pad_to` usando reflexion.
fn padded_canvas(source: &DecodedImage, pad_to: u32) -> SuResult<Vec<f32>> {
    let pad_to = pad_to.max(1);
    let canvas_width = source.width.div_ceil(pad_to) * pad_to;
    let canvas_height = source.height.div_ceil(pad_to) * pad_to;

    if canvas_width == source.width && canvas_height == source.height {
        return Ok(source.rgb.clone());
    }

    let mut canvas = vec![0.0f32; (canvas_width as usize) * (canvas_height as usize) * 3];

    for y in 0..canvas_height {
        let source_y = reflect_index(y as i64, source.height);
        for x in 0..canvas_width {
            let source_x = reflect_index(x as i64, source.width);

            let source_index = ((source_y as usize) * (source.width as usize) + source_x as usize) * 3;
            let target_index = ((y as usize) * (canvas_width as usize) + x as usize) * 3;

            canvas[target_index..target_index + 3]
                .copy_from_slice(&source.rgb[source_index..source_index + 3]);
        }
    }

    Ok(canvas)
}

/// Indice reflejado, como un espejo que rebota en los bordes.
///
/// Se usa aritmetica modular en lugar de un bucle: un tile puede quedar muy
/// fuera del borde si el usuario fuerza un tamano grande, y un bucle con indices
/// negativos es una fuente clasica de cuelgues.
pub fn reflect_index(index: i64, length: u32) -> u32 {
    if length <= 1 {
        return 0;
    }
    let length = length as i64;
    let period = 2 * (length - 1);

    let mut value = index % period;
    if value < 0 {
        value += period;
    }
    if value >= length {
        value = period - value;
    }
    value as u32
}

/// Copia la region de un tile desde el canvas ya rellenado.
///
/// Devuelve un buffer propio en lugar de una vista prestada. El motivo es que los
/// tiles que no empiezan en la columna 0 no son contiguos en memoria: devolver un
/// slice exigiria que lo fueran, y devolver algo mal formado en ese caso seria
/// justo el tipo de fallo silencioso que el proyecto quiere evitar. Ademas, un
/// backend real tiene que construir el tensor de entrada de todas formas.
fn extract_tile(canvas: &[f32], plan: &TilePlan, tile: &su_tiling::Tile) -> Vec<f32> {
    const CHANNELS: usize = 3;

    let tile_width = tile.read_w as usize;
    let tile_height = tile.read_h as usize;
    let canvas_width = plan.canvas_width as usize;

    let mut data = vec![0.0f32; tile_width * tile_height * CHANNELS];

    for row in 0..tile_height {
        let source_start =
            ((tile.canvas_y as usize + row) * canvas_width + tile.canvas_x as usize) * CHANNELS;
        let source_end = source_start + tile_width * CHANNELS;
        let target_start = row * tile_width * CHANNELS;

        data[target_start..target_start + tile_width * CHANNELS]
            .copy_from_slice(&canvas[source_start..source_end]);
    }

    data
}

/// Aplica el modelo a la imagen completa, sin trocear.
///
/// Se usa para denoise y restauracion facial: su contexto es global y trocearlos
/// produciria costuras en las zonas de transicion.
fn full_image_pass(
    source: &DecodedImage,
    backend: &mut dyn Backend,
    config: &RunnerConfig,
) -> SuResult<DecodedImage> {
    let pad_to = config.pad_to.max(1);
    let tile = source.width.max(source.height).max(MIN_TILE);
    let plan = TilePlan::new(source.width, source.height, tile, 0, pad_to)?;
    let canvas = padded_canvas(source, pad_to)?;

    let data = composite(&plan, 1, 3, |current| {
        let buffer = extract_tile(&canvas, &plan, current);
        let input = TileInput::new(current.read_w, current.read_h, 3, &buffer)?;
        let output = backend.run_tile(&input)?;
        Ok(output.data)
    })?;

    Ok(DecodedImage {
        width: source.width,
        height: source.height,
        rgb: data,
        alpha: None,
        icc_profile: source.icc_profile.clone(),
        applied_orientation: source.applied_orientation,
    })
}

/// Enfoque unsharp-mask.
///
/// `out = src + amount * (src - blur(src))`, aplicando el umbral para no amplificar
/// el ruido de las zonas planas. El desenfoque es una caja separable aplicada tres
/// veces, que aproxima una gaussiana con mucho menos codigo que un kernel real.
pub fn unsharp(
    source: &DecodedImage,
    amount: f32,
    radius: f32,
    threshold: f32,
) -> SuResult<DecodedImage> {
    if amount.is_nan() || amount <= 0.0 || radius.is_nan() || radius <= 0.0 {
        return Ok(source.clone());
    }

    let blurred = box_blur_rgb(&source.rgb, source.width, source.height, radius, 3);
    // El umbral va en la misma escala que el resto de los campos del pipeline
    // —`amount`, `blend`, `factor`—: 0..1. Antes se dividia por 255, asi que el
    // `threshold: 0.02` que declaran los pipelines de foto valia 0,0000784 y no
    // filtraba nada: el enfoque amplificaba el ruido de las zonas planas, que es
    // justo lo que el umbral existe para evitar, y la diferencia entre el 0,03 del
    // dibujo y el 0,02 de la foto no producia ningun efecto observable.
    let threshold = threshold.max(0.0);

    let mut rgb = source.rgb.clone();
    for (target, (original, soft)) in rgb.iter_mut().zip(source.rgb.iter().zip(blurred.iter())) {
        let detail = original - soft;
        if detail.abs() < threshold {
            continue;
        }
        *target = (original + amount * detail).clamp(0.0, 1.0);
    }

    Ok(DecodedImage {
        rgb,
        ..source.clone()
    })
}

/// Desenfoque de caja separable, repetido `passes` veces.
fn box_blur_rgb(data: &[f32], width: u32, height: u32, radius: f32, passes: u32) -> Vec<f32> {
    let channels = 3usize;
    let window = (radius.round() as usize).max(1);
    let mut current = data.to_vec();
    let mut scratch = vec![0.0f32; data.len()];

    for _ in 0..passes.max(1) {
        blur_horizontal(&current, &mut scratch, width, height, channels, window);
        blur_vertical(&scratch, &mut current, width, height, channels, window);
    }

    current
}

fn blur_horizontal(
    source: &[f32],
    target: &mut [f32],
    width: u32,
    height: u32,
    channels: usize,
    window: usize,
) {
    let width_usize = width as usize;
    let span = (2 * window + 1) as f32;

    for y in 0..height as usize {
        let row = y * width_usize;
        for x in 0..width_usize {
            for channel in 0..channels {
                let mut total = 0.0f32;
                for offset in 0..=(2 * window) {
                    let sample = x as i64 + offset as i64 - window as i64;
                    let index = reflect_index(sample, width) as usize;
                    total += source[(row + index) * channels + channel];
                }
                target[(row + x) * channels + channel] = total / span;
            }
        }
    }
}

fn blur_vertical(
    source: &[f32],
    target: &mut [f32],
    width: u32,
    height: u32,
    channels: usize,
    window: usize,
) {
    let width_usize = width as usize;
    let span = (2 * window + 1) as f32;

    for y in 0..height as usize {
        for x in 0..width_usize {
            for channel in 0..channels {
                let mut total = 0.0f32;
                for offset in 0..=(2 * window) {
                    let sample = y as i64 + offset as i64 - window as i64;
                    let row = reflect_index(sample, height) as usize;
                    total += source[(row * width_usize + x) * channels + channel];
                }
                target[(y * width_usize + x) * channels + channel] = total / span;
            }
        }
    }
}
