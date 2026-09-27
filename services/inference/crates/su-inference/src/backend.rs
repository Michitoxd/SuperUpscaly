//! Backend de inferencia: la frontera entre el pipeline y el runtime real.
//!
//! ## Por que existe esta abstraccion
//!
//! El plan original ponia ONNX Runtime en el centro. En la practica eso obliga a
//! tener un modelo, un EP funcional y cientos de megabytes de bibliotecas solo
//! para poder comprobar que la composicion de tiles no deja costuras.
//!
//! Con esta frontera:
//!
//! - El pipeline, el tiling y la degradacion se testean con `MockBackend`, que es
//!   aritmetica pura y no necesita nada.
//! - `su-cli` puede ejecutar un lote completo sin ORT instalado (util para CI y
//!   para que el usuario compruebe el flujo antes de descargar modelos).
//! - Cambiar de runtime (ONNX, TensorRT nativo, lo que venga) no toca ni el motor
//!   de pipelines ni la planificacion de tiles.
//!
//! ## Contrato
//!
//! Un backend recibe **un tile** en `f32` entrelazado y devuelve **ese tile
//! escalado**. No sabe nada de solapes, de VRAM ni de la imagen completa: de eso
//! se encarga `su-tiling`. Esta separacion es la que hace que la composicion sea
//! testeable de forma aislada.

use su_core::{SuError, SuResult};

/// Entrada de un tile: pixeles en `f32`, entrelazados, en `0..1`.
#[derive(Debug, Clone, Copy)]
pub struct TileInput<'a> {
    pub width: u32,
    pub height: u32,
    pub channels: usize,
    pub data: &'a [f32],
}

impl<'a> TileInput<'a> {
    pub fn new(width: u32, height: u32, channels: usize, data: &'a [f32]) -> SuResult<Self> {
        let expected = (width as usize) * (height as usize) * channels;
        if data.len() != expected {
            return Err(SuError::TileFailed {
                row: 0,
                col: 0,
                reason: format!(
                    "entrada de {} valores, se esperaban {expected}",
                    data.len()
                ),
            });
        }
        Ok(Self {
            width,
            height,
            channels,
            data,
        })
    }

    pub fn pixel_count(&self) -> usize {
        (self.width as usize) * (self.height as usize)
    }
}

/// Resultado de ejecutar un modelo sobre un tile.
#[derive(Debug, Clone, PartialEq)]
pub struct TileOutput {
    pub width: u32,
    pub height: u32,
    pub channels: usize,
    pub data: Vec<f32>,
}

impl TileOutput {
    pub fn new(width: u32, height: u32, channels: usize, data: Vec<f32>) -> SuResult<Self> {
        let expected = (width as usize) * (height as usize) * channels;
        if data.len() != expected {
            return Err(SuError::Internal(format!(
                "salida de {} valores, se esperaban {expected}",
                data.len()
            )));
        }
        Ok(Self {
            width,
            height,
            channels,
            data,
        })
    }
}

/// Modelo cargado y listo para ejecutar.
///
/// Exige `Debug` porque un backend se guarda detras de `Box<dyn Backend>`: sin
/// esto, `Result<Box<dyn Backend>, SuError>` no se puede imprimir ni inspeccionar
/// en un test o en un log, y el backend elegido es justo el dato que hace falta
/// cuando un resultado no cuadra.
pub trait Backend: Send + std::fmt::Debug {
    /// Identificador del modelo o del backend, para logs y para el resumen final.
    fn id(&self) -> &str;

    /// Factor de escala nativo. 1 para modelos que no escalan.
    fn scale(&self) -> u32;

    /// VRAM consumida por megapixel de tile, si el backend la conoce.
    ///
    /// Devolver `None` hace que el planificador use un valor conservador: mejor
    /// ir lento la primera vez que arriesgar un desbordamiento.
    fn vram_per_megapixel(&self) -> Option<f32> {
        None
    }

    /// Ejecuta el modelo sobre un tile.
    fn run_tile(&mut self, input: &TileInput<'_>) -> SuResult<TileOutput>;

    /// Si el backend admite ejecutar el modelo sobre la imagen completa.
    ///
    /// Los modelos de denoise y de restauracion facial no se trocean: se aplican
    /// a la imagen entera porque su contexto es global. Los que devuelven `false`
    /// se omiten y se reportan como omitidos, nunca en silencio.
    fn supports_full_image(&self) -> bool {
        true
    }
}

/// Backend de referencia: reescalado por vecino mas cercano.
///
/// No sintetiza detalle, pero **si** reescala con la geometria exacta del modelo
/// que sustituye. Eso lo convierte en el patron de referencia para verificar que
/// el tiling compone bien: si `MockBackend` deja una costura, el fallo esta en la
/// composicion y no en el modelo.
#[derive(Debug, Clone)]
pub struct MockBackend {
    id: String,
    scale: u32,
    vram_per_megapixel: Option<f32>,
    full_image: bool,
}

impl MockBackend {
    pub fn new(scale: u32) -> SuResult<Self> {
        if scale == 0 || scale > 8 {
            return Err(SuError::Internal(format!("escala de mock invalida: {scale}")));
        }
        Ok(Self {
            id: format!("mock-x{scale}"),
            scale,
            vram_per_megapixel: None,
            full_image: true,
        })
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    pub fn with_vram_per_megapixel(mut self, value: f32) -> Self {
        self.vram_per_megapixel = Some(value);
        self
    }

    pub fn without_full_image(mut self) -> Self {
        self.full_image = false;
        self
    }
}

impl Backend for MockBackend {
    fn id(&self) -> &str {
        &self.id
    }

    fn scale(&self) -> u32 {
        self.scale
    }

    fn vram_per_megapixel(&self) -> Option<f32> {
        self.vram_per_megapixel
    }

    fn supports_full_image(&self) -> bool {
        self.full_image
    }

    fn run_tile(&mut self, input: &TileInput<'_>) -> SuResult<TileOutput> {
        let out_width = input.width * self.scale;
        let out_height = input.height * self.scale;
        let channels = input.channels;

        let mut data = vec![0.0f32; (out_width as usize) * (out_height as usize) * channels];

        for out_y in 0..out_height {
            let source_y = out_y / self.scale;
            for out_x in 0..out_width {
                let source_x = out_x / self.scale;

                let source = ((source_y as usize) * (input.width as usize) + source_x as usize)
                    * channels;
                let target =
                    ((out_y as usize) * (out_width as usize) + out_x as usize) * channels;

                data[target..target + channels]
                    .copy_from_slice(&input.data[source..source + channels]);
            }
        }

        TileOutput::new(out_width, out_height, channels, data)
    }
}

/// Backend clasico: interpolacion de alta calidad, en `f32` y sin modelos.
///
/// ## Por que existe
///
/// El proyecto tiene dos extremos: [`MockBackend`], que solo sirve para verificar
/// geometria, y `OrtBackend`, que es el que da calidad de verdad pero necesita
/// ONNX Runtime, un modelo descargado y (si hay suerte) una GPU. Entre los dos
/// faltaba lo que ocurre **casi siempre**: no hay modelo instalado todavia, o el
/// runtime no arranca en esta maquina.
///
/// Hasta ahora ese hueco lo cubria `MockBackend`, que es vecino mas cercano: en
/// una imagen de 128 px a x4 cada pixel se convierte en un bloque de 4x4, con
/// escalones de cuatro pixeles en cada diagonal. Y peor: el resto del pipeline
/// cuenta con que el backend devuelve una **interpolacion** (las etapas de
/// restauracion suben a x4 y vuelven a bajar con un filtro), asi que con vecino
/// mas cercano el ida y vuelta no reconstruia nada y devolvia una imagen lavada:
/// los negros puros salian grises y los bordes, emborronados.
///
/// Este backend cierra ese hueco con lo mejor que se puede hacer sin un modelo:
/// un interpolador de reconstruccion de verdad (Lanczos3 por defecto), en coma
/// flotante y sin cuantizar. No inventa detalle que no existe, pero **no lo
/// destruye**: los contornos quedan nitidos y las zonas planas no se ensucian.
///
/// El nombre del backend se propaga al informe del trabajo, de modo que el
/// usuario sabe siempre si el resultado lo hizo un modelo o un interpolador.
#[derive(Debug, Clone)]
pub struct ClassicalBackend {
    id: String,
    scale: u32,
    kernel: su_imageio::ResizeKernel,
}

impl ClassicalBackend {
    pub fn new(scale: u32, kernel: su_imageio::ResizeKernel) -> SuResult<Self> {
        if scale == 0 || scale > 8 {
            return Err(SuError::Internal(format!("escala clasica invalida: {scale}")));
        }
        Ok(Self {
            id: format!("clasico-{}", kernel.as_str()),
            scale,
            kernel,
        })
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    pub fn kernel(&self) -> su_imageio::ResizeKernel {
        self.kernel
    }
}

impl Backend for ClassicalBackend {
    fn id(&self) -> &str {
        &self.id
    }

    fn scale(&self) -> u32 {
        self.scale
    }

    fn run_tile(&mut self, input: &TileInput<'_>) -> SuResult<TileOutput> {
        // El interpolador recibe un bloque de pixeles y devuelve ese bloque
        // escalado. El solape lo resuelve el compositor, igual que con un modelo.
        let source = su_imageio::DecodedImage {
            width: input.width,
            height: input.height,
            rgb: input.data.to_vec(),
            alpha: None,
            icc_profile: None,
            applied_orientation: 1,
        };

        let scaled = su_imageio::resize(
            &source,
            input.width * self.scale,
            input.height * self.scale,
            self.kernel,
        )?;

        TileOutput::new(
            scaled.width,
            scaled.height,
            input.channels,
            scaled.rgb,
        )
    }
}

/// Backend que solo aplica una identidad. Util para comprobar la ruta de
/// composicion sin escalar nada.
#[derive(Debug, Clone, Default)]
pub struct IdentityBackend;

impl Backend for IdentityBackend {
    fn id(&self) -> &str {
        "identity"
    }

    fn scale(&self) -> u32 {
        1
    }

    fn run_tile(&mut self, input: &TileInput<'_>) -> SuResult<TileOutput> {
        TileOutput::new(
            input.width,
            input.height,
            input.channels,
            input.data.to_vec(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_upscale_multiplies_the_dimensions() {
        let mut backend = MockBackend::new(4).expect("mock");
        let input = TileInput::new(2, 3, 3, &[0.0; 18]).expect("entrada");

        let output = backend.run_tile(&input).expect("ejecucion");
        assert_eq!((output.width, output.height), (8, 12));
        assert_eq!(output.data.len(), 8 * 12 * 3);
    }

    #[test]
    fn mock_upscale_replicates_each_pixel() {
        let mut backend = MockBackend::new(2).expect("mock");
        // 2x1 con un pixel rojo y otro verde.
        let data = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let input = TileInput::new(2, 1, 3, &data).expect("entrada");

        let output = backend.run_tile(&input).expect("ejecucion");
        assert_eq!((output.width, output.height), (4, 2));

        // El pixel (0,0) y su vecino (1,0) deben ser rojos.
        assert_eq!(&output.data[0..3], &[1.0, 0.0, 0.0]);
        assert_eq!(&output.data[3..6], &[1.0, 0.0, 0.0]);
        // Y en la fila de abajo, igual.
        assert_eq!(&output.data[12..15], &[1.0, 0.0, 0.0]);
    }

    #[test]
    fn an_identity_backend_preserves_the_pixels() {
        let mut backend = IdentityBackend;
        let data: Vec<f32> = (0..24).map(|value| value as f32 / 24.0).collect();
        let input = TileInput::new(2, 4, 3, &data).expect("entrada");

        let output = backend.run_tile(&input).expect("ejecucion");
        assert_eq!(output.data, data);
        assert_eq!(backend.scale(), 1);
    }

    #[test]
    fn an_invalid_scale_is_rejected() {
        assert!(MockBackend::new(0).is_err());
        assert!(MockBackend::new(16).is_err());
    }

    #[test]
    fn tile_input_rejects_a_buffer_of_the_wrong_size() {
        let error = TileInput::new(4, 4, 3, &[0.0; 10]).unwrap_err();
        assert_eq!(error.code().as_str(), "SU-E140");
    }

    #[test]
    fn tile_output_rejects_a_buffer_of_the_wrong_size() {
        assert!(TileOutput::new(4, 4, 3, vec![0.0; 10]).is_err());
        assert!(TileOutput::new(4, 4, 3, vec![0.0; 48]).is_ok());
    }

    #[test]
    fn the_backend_id_describes_the_scale() {
        assert_eq!(MockBackend::new(4).unwrap().id(), "mock-x4");
        assert_eq!(
            MockBackend::new(2).unwrap().with_id("4x-ultrasharp").id(),
            "4x-ultrasharp"
        );
    }

    #[test]
    fn the_classical_backend_scales_with_interpolation_not_with_blocks() {
        // El vecino mas cercano no puede inventar valores: devuelve los mismos que
        // la entrada, cada uno repetido en un bloque. Un interpolador, en cambio,
        // crea valores intermedios que antes no existian. Contar valores distintos
        // es la forma mas directa de distinguir los dos comportamientos sin
        // depender de un valor concreto de la tabla de filtros.
        let side = 6u32;
        let mut data = Vec::new();
        for _y in 0..side {
            for x in 0..side {
                let value = x as f32 / (side - 1) as f32;
                data.extend_from_slice(&[value, value, value]);
            }
        }

        let mut classical = ClassicalBackend::new(4, su_imageio::ResizeKernel::Lanczos3).unwrap();
        let mut nearest = MockBackend::new(4).unwrap();

        let input = TileInput::new(side, side, 3, &data).unwrap();
        let interpolated = classical.run_tile(&input).unwrap();
        let blocked = nearest.run_tile(&input).unwrap();

        assert_eq!(interpolated.width, side * 4);
        assert_eq!(blocked.width, side * 4);

        let distinct = |values: &[f32]| {
            let mut seen: Vec<u32> = values
                .chunks_exact(3)
                .map(|pixel| (pixel[0] * 1_000_000.0).round() as u32)
                .collect();
            seen.sort_unstable();
            seen.dedup();
            seen.len()
        };

        assert_eq!(
            distinct(&blocked.data),
            side as usize,
            "el vecino mas cercano solo puede repetir los valores de la entrada"
        );
        assert!(
            distinct(&interpolated.data) > side as usize,
            "el interpolador deberia crear valores intermedios, y solo hay {} distintos",
            distinct(&interpolated.data)
        );
    }

    #[test]
    fn the_classical_backend_stays_in_range_and_keeps_the_size() {
        let mut backend = ClassicalBackend::new(2, su_imageio::ResizeKernel::CatmullRom).unwrap();
        let input = TileInput::new(4, 4, 3, &[0.9f32; 48]).unwrap();
        let output = backend.run_tile(&input).unwrap();

        assert_eq!((output.width, output.height), (8, 8));
        assert_eq!(output.data.len(), 8 * 8 * 3);
        for value in &output.data {
            assert!((-0.001..=1.001).contains(value), "fuera de rango: {value}");
        }
    }

    #[test]
    fn the_classical_backend_does_not_change_anything_at_scale_one() {
        // Es el caso de las etapas de restauracion: suben a x4 con el modelo y
        // vuelven a bajar. A escala 1 no hay nada que hacer, y hacerlo costaba la
        // nitidez de la imagen entera.
        let mut backend = ClassicalBackend::new(1, su_imageio::ResizeKernel::Lanczos3).unwrap();
        let data: Vec<f32> = (0..48).map(|value| value as f32 / 48.0).collect();
        let input = TileInput::new(4, 4, 3, &data).unwrap();

        let output = backend.run_tile(&input).unwrap();
        assert_eq!(output.data, data);
    }

    #[test]
    fn an_invalid_classical_scale_is_rejected() {
        assert!(ClassicalBackend::new(0, su_imageio::ResizeKernel::Lanczos3).is_err());
        assert!(ClassicalBackend::new(9, su_imageio::ResizeKernel::Lanczos3).is_err());
    }

    #[test]
    fn vram_hint_is_optional_and_reported_when_set() {
        assert_eq!(MockBackend::new(4).unwrap().vram_per_megapixel(), None);
        assert_eq!(
            MockBackend::new(4).unwrap().with_vram_per_megapixel(420.0).vram_per_megapixel(),
            Some(420.0)
        );
    }
}
