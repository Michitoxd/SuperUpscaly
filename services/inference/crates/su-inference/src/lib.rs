//! # su-inference
//!
//! Ejecucion de pipelines: une el motor declarativo de `su-core`, la
//! planificacion de tiles de `su-tiling` y un backend de inferencia.
//!
//! ## ONNX Runtime, detras de una feature
//!
//! La implementacion con ORT vive en [`ort_backend`], compilada solo con la
//! feature `onnx`. El motivo no es comodidad de desarrollo: significa que el
//! motor de pipelines, el tiling y la degradacion se pueden **verificar sin GPU,
//! sin modelos y sin ORT**, que es donde estan los errores que de verdad importan
//! (costuras, geometria, bucles de reintento).
//!
//! ```bash
//! cargo test                                  # sin ORT
//! cargo build --features onnx                 # con ORT
//! ```
//!
//! [`MockBackend`] cubre el hueco de las pruebas: reescala con la geometria
//! exacta del modelo al que sustituye, asi que sirve para verificar la
//! composicion, no la calidad del resultado. Para lo que ocurre en produccion sin
//! modelos esta [`ClassicalBackend`], que interpola de verdad.

pub mod backend;
pub mod runner;

#[cfg(feature = "onnx")]
pub mod ort_backend;

pub use backend::{
    Backend, ClassicalBackend, IdentityBackend, MockBackend, TileInput, TileOutput,
};
pub use runner::{
    choose_tile_without_budget, reflect_index, run_pipeline, scale_from_model_id, unsharp,
    BackendProvider, ClassicalProvider, MockBackendProvider, RunOutcome, RunnerConfig,
    StageProgress, TilingHints,
};

#[cfg(feature = "onnx")]
pub use ort_backend::{
    find_runtime_library, from_nchw, pick_runtime_library, probe_runtime, to_nchw, OrtBackend,
    OrtConfig, OrtProvider,
};

#[cfg(test)]
mod tests {
    use super::*;
    use su_core::{EvalVars, Mode, PipelineSet, Scale, StageId, SuError, TileChoice};
    use su_imageio::DecodedImage;
    use su_tiling::VramBudget;

    fn gradient(width: u32, height: u32) -> DecodedImage {
        let mut rgb = Vec::with_capacity((width as usize) * (height as usize) * 3);
        for y in 0..height {
            for x in 0..width {
                rgb.push(x as f32 / width as f32);
                rgb.push(y as f32 / height as f32);
                rgb.push(0.25);
            }
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

    fn vars_for(mode: Mode) -> EvalVars {
        EvalVars {
            mode,
            noise: 0.05,
            blockiness: 0.05,
            face_count: 0,
            megapixels: 1.0,
            estimated_output_mp: 16.0,
            denoise: su_core::DenoiseChoice::Off,
            face_restore: su_core::FaceRestoreChoice::Off,
            sharpen: false,
            ..EvalVars::default()
        }
    }

    fn photo_pipeline() -> su_core::Pipeline {
        PipelineSet::embedded()
            .expect("pipelines embebidos")
            .get(Mode::Photo, Scale::X4)
            .expect("pipeline photo:4x")
            .clone()
    }

    fn run_with(
        image: &DecodedImage,
        pipeline: &su_core::Pipeline,
        vars: &EvalVars,
        config: &RunnerConfig,
    ) -> RunOutcome {
        let provider = MockBackendProvider;
        let mut progress_calls = 0u32;
        run_pipeline(image, pipeline, vars, &provider, config, &mut |_| {
            progress_calls += 1;
        })
        .expect("ejecucion del pipeline")
    }

    /// Ejecuta el pipeline y devuelve el error, para los casos que deben fallar.
    fn run_err(
        image: &DecodedImage,
        pipeline: &su_core::Pipeline,
        vars: &EvalVars,
    ) -> SuError {
        let provider = MockBackendProvider;
        run_pipeline(
            image,
            pipeline,
            vars,
            &provider,
            &RunnerConfig::default(),
            &mut |_| {},
        )
        .expect_err("el pipeline deberia haber fallado")
    }

    /// Etapa `model` con los campos neutros, para no repetir veinte lineas por
    /// etapa en cada test.
    fn model_stage(id: &str, model: &str, scale_out: u32) -> su_core::Stage {
        su_core::Stage {
            id: id.to_string(),
            op: su_core::StageOp::Model,
            model: Some(model.to_string()),
            fallback_model: None,
            when: None,
            tiling: None,
            scale_out: Some(scale_out),
            blend: None,
            blend_from: None,
            only_on_faces: false,
            overlap_boost: 0,
            kernel: None,
            factor: None,
            amount: None,
            radius: None,
            threshold: None,
        }
    }

    /// Backend de escala 1 que devuelve un color plano.
    ///
    /// Es lo que hace falta para ver **donde** ha llegado el parche de la
    /// restauracion facial: con un backend que devuelve lo mismo que recibe, el
    /// pegado seria invisible y la prueba no comprobaria nada.
    #[derive(Debug)]
    struct FlatColorBackend {
        color: [f32; 3],
    }

    impl Backend for FlatColorBackend {
        fn id(&self) -> &str {
            "cara-plana"
        }

        fn scale(&self) -> u32 {
            1
        }

        fn run_tile(&mut self, input: &TileInput<'_>) -> su_core::SuResult<TileOutput> {
            let mut data = vec![0.0f32; input.pixel_count() * 3];
            for pixel in data.chunks_exact_mut(3) {
                pixel.copy_from_slice(&self.color);
            }
            TileOutput::new(input.width, input.height, 3, data)
        }
    }

    /// Color del pixel (x, y) de una imagen.
    fn pixel(image: &DecodedImage, x: u32, y: u32) -> [f32; 3] {
        let start = ((y as usize) * (image.width as usize) + x as usize) * 3;
        [
            image.rgb[start],
            image.rgb[start + 1],
            image.rgb[start + 2],
        ]
    }

    fn face_box(x: f32, y: f32, w: f32, h: f32, confidence: f32) -> su_core::FaceBox {
        su_core::FaceBox {
            x,
            y,
            w,
            h,
            confidence,
        }
    }

    #[test]
    fn overlapping_face_boxes_describe_one_face() {
        // Dos cajas casi iguales (el mismo rostro detectado dos veces) se funden.
        let merged = crate::runner::merge_face_boxes(&[
            face_box(0.40, 0.40, 0.10, 0.10, 0.9),
            face_box(0.41, 0.41, 0.10, 0.10, 0.8),
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].confidence, 0.9);

        // Y dos caras separadas siguen siendo dos.
        let separate = crate::runner::merge_face_boxes(&[
            face_box(0.10, 0.10, 0.10, 0.10, 0.9),
            face_box(0.60, 0.60, 0.10, 0.10, 0.9),
        ]);
        assert_eq!(separate.len(), 2);

        // Una caja con poca confianza no es una cara.
        let unreliable = crate::runner::merge_face_boxes(&[face_box(0.1, 0.1, 0.2, 0.2, 0.2)]);
        assert!(unreliable.is_empty());
    }

    #[test]
    fn the_face_crop_has_margin_and_never_leaves_the_image() {
        let centered = face_box(0.45, 0.45, 0.10, 0.10, 0.9);
        let (left, top, side) = crate::runner::face_crop_rect(&centered, 1000, 1000);
        // 100 px de cara con un 35% de margen a cada lado.
        assert_eq!(side, 170);
        assert_eq!((left, top), (415, 415));

        // Pegada a la esquina: el recorte se desplaza para caber entero.
        let corner = face_box(0.0, 0.0, 0.10, 0.10, 0.9);
        let (left, top, side) = crate::runner::face_crop_rect(&corner, 1000, 1000);
        assert_eq!((left, top), (0, 0), "el recorte tiene que caber en la imagen");
        assert_eq!(side, 170);

        // Y en una imagen mas pequena que el recorte pedido, manda la imagen.
        let large = face_box(0.20, 0.20, 0.60, 0.80, 0.9);
        let (left, top, side) = crate::runner::face_crop_rect(&large, 120, 80);
        assert_eq!(side, 80, "el recorte no puede ser mayor que la imagen");
        assert!(left + side <= 120 && top + side <= 80);
    }

    #[test]
    fn face_restoration_touches_the_face_and_its_margin_only() {
        // Imagen 512x512 con una cara de 100 px en el centro: el recorte son 170 px.
        let source = gradient(512, 512);
        let faces = [face_box(0.40, 0.40, 0.20, 0.20, 0.9)];

        let mut backend = FlatColorBackend {
            color: [1.0, 0.0, 0.0],
        };
        let outcome = crate::runner::restore_faces(
            &source,
            &faces,
            1.0,
            &mut backend,
            StageId::Face,
            &mut |_| {},
        )
        .expect("restauracion");

        assert_eq!(outcome.processed, 1);
        assert_eq!(outcome.too_small, 0);
        assert_eq!(outcome.image.width, source.width);

        // El centro de la cara es el color que devolvio el modelo...
        let center = pixel(&outcome.image, 256, 256);
        assert!(
            center[0] > 0.95 && center[1] < 0.05,
            "centro de la cara: {center:?}"
        );

        // ...y una esquina de la imagen no se ha tocado.
        assert_eq!(pixel(&outcome.image, 0, 0), pixel(&source, 0, 0));
        assert_eq!(pixel(&outcome.image, 511, 511), pixel(&source, 511, 511));

        // Propiedad: la transicion del margen es parcial, ni cero ni uno. Es lo que
        // separa un pegado con mascara de un parche rectangular.
        // El recorte va de 169 a 343, asi que la transicion del margen esta en sus
        // ultimos pixeles (el centro de la cara llega hasta el radio interior).
        let margin = pixel(&outcome.image, 256, 340);
        let original = pixel(&source, 256, 340);
        assert!(
            margin[0] > original[0] && margin[0] < 1.0,
            "margen: {margin:?}, original: {original:?}"
        );
    }

    #[test]
    fn a_face_too_small_for_the_model_is_left_alone_and_reported() {
        let source = gradient(128, 128);
        let faces = [face_box(0.40, 0.40, 0.20, 0.20, 0.9)];

        let mut backend = FlatColorBackend {
            color: [1.0, 0.0, 0.0],
        };
        let outcome = crate::runner::restore_faces(
            &source,
            &faces,
            1.0,
            &mut backend,
            StageId::Face,
            &mut |_| {},
        )
        .expect("restauracion");

        assert_eq!(outcome.processed, 0);
        assert_eq!(outcome.too_small, 1);
        assert_eq!(pixel(&outcome.image, 64, 64), pixel(&source, 64, 64));
    }

    #[test]
    fn reflect_index_bounces_off_the_edges() {
        // Longitud 4: los indices validos son 0..3.
        assert_eq!(reflect_index(0, 4), 0);
        assert_eq!(reflect_index(3, 4), 3);
        assert_eq!(reflect_index(-1, 4), 1);
        assert_eq!(reflect_index(4, 4), 2);
        assert_eq!(reflect_index(5, 4), 1);
        assert_eq!(reflect_index(6, 4), 0);
        // Nunca se sale del rango, por muy lejos que se vaya.
        for index in -50i64..50 {
            assert!(reflect_index(index, 4) < 4, "indice {index}");
        }
    }

    #[test]
    fn reflect_index_handles_degenerate_lengths() {
        assert_eq!(reflect_index(0, 0), 0);
        assert_eq!(reflect_index(5, 1), 0);
    }

    #[test]
    fn the_pipeline_produces_the_requested_scale() {
        let image = gradient(200, 120);
        let outcome = run_with(
            &image,
            &photo_pipeline(),
            &vars_for(Mode::Photo),
            &RunnerConfig::default(),
        );

        assert_eq!(outcome.image.width, 800);
        assert_eq!(outcome.image.height, 480);
        assert_eq!(outcome.image.rgb.len(), 800 * 480 * 3);
    }

    #[test]
    fn geometry_survives_tiling() {
        // Con el backend de vecino mas cercano, cada pixel de origen debe
        // aparecer replicado exactamente en su bloque. Si el tiling desplazara la
        // geometria, esta comprobacion fallaria.
        let image = gradient(300, 180);
        let config = RunnerConfig {
            // Tile pequeno a proposito: fuerza varios tiles por eje.
            tile_choice: TileChoice::Px256,
            ..RunnerConfig::default()
        };

        let outcome = run_with(&image, &photo_pipeline(), &vars_for(Mode::Photo), &config);

        // El indice de destino se calcula con el ancho **real** de la salida, no con
        // una constante. Con el ancho escrito a mano, el test leia otra fila de la
        // imagen y acusaba al tiling de un desplazamiento que no existia: 300 x 4
        // son 1200 columnas, no 800.
        assert_eq!(outcome.image.width, 1200);
        assert_eq!(outcome.image.height, 720);
        let out_width = outcome.image.width as usize;

        for (x, y) in [(0u32, 0u32), (7, 5), (150, 90), (299, 179), (299, 0), (0, 179)] {
            let source_index = ((y as usize) * 300 + x as usize) * 3;
            let out_x = x * 4;
            let out_y = y * 4;
            let target_index = ((out_y as usize) * out_width + out_x as usize) * 3;

            for channel in 0..3 {
                let expected = image.rgb[source_index + channel];
                let actual = outcome.image.rgb[target_index + channel];
                assert!(
                    (expected - actual).abs() < 1e-6,
                    "pixel ({x},{y}) canal {channel}: {expected} != {actual}"
                );
            }
        }
    }

    #[test]
    fn output_is_not_corrupted_by_composition() {
        // Sin costuras: ningun pixel puede salir a 0 (que es lo que produce un
        // hueco de cobertura) ni fuera de rango.
        let image = gradient(250, 150);
        let outcome = run_with(
            &image,
            &photo_pipeline(),
            &vars_for(Mode::Photo),
            &RunnerConfig::default(),
        );

        for (index, value) in outcome.image.rgb.iter().enumerate() {
            assert!(value.is_finite(), "valor no finito en {index}");
            assert!(
                (-0.001..=1.001).contains(value),
                "valor fuera de rango en {index}: {value}"
            );
        }
    }

    #[test]
    fn the_result_passes_output_validation() {
        let image = gradient(200, 200);
        let outcome = run_with(
            &image,
            &photo_pipeline(),
            &vars_for(Mode::Photo),
            &RunnerConfig::default(),
        );
        assert!(su_imageio::validate_output(&outcome.image).is_ok());
    }

    #[test]
    fn the_expected_stages_are_reported_as_executed() {
        let image = gradient(120, 120);
        let outcome = run_with(
            &image,
            &photo_pipeline(),
            &vars_for(Mode::Photo),
            &RunnerConfig::default(),
        );

        assert!(outcome.executed.contains(&StageId::Analyze));
        assert!(outcome.executed.contains(&StageId::Upscale));
        // Con denoise, cara y enfoque desactivados, no deben aparecer.
        assert!(!outcome.executed.contains(&StageId::Denoise));
        assert!(!outcome.executed.contains(&StageId::Face));
        assert!(!outcome.executed.contains(&StageId::Sharpen));
    }

    #[test]
    fn enabling_conditions_adds_stages() {
        let image = gradient(120, 120);
        let mut vars = vars_for(Mode::Photo);
        vars.noise = 0.9;
        vars.denoise = su_core::DenoiseChoice::On;
        // Una cara con su caja: desde que la etapa facial recorta lo que declara, el
        // numero de caras no basta para que la etapa tenga algo que hacer.
        vars.face_count = 1;
        vars.faces = vec![face_box(0.25, 0.25, 0.5, 0.5, 0.9)];
        vars.face_restore = su_core::FaceRestoreChoice::Auto;
        vars.sharpen = true;

        let outcome = run_with(&image, &photo_pipeline(), &vars, &RunnerConfig::default());

        assert!(outcome.executed.contains(&StageId::Denoise));
        assert!(outcome.executed.contains(&StageId::Face));
        assert!(outcome.executed.contains(&StageId::Sharpen));
    }

    #[test]
    fn the_denoise_weight_follows_the_measured_noise() {
        // MEJORA-01, medido en el resultado y no en la configuracion: el mismo
        // pipeline, sobre la misma imagen, con la misma preferencia de denoise, tiene
        // que dar resultados **distintos** si el analisis mide ruido moderado o ruido
        // alto. Con un peso fijo (el antiguo `blend: 0.9`) salian identicos, asi que
        // el ajuste de denoise era el mismo para una foto limpia que para una
        // degradada.
        let image = gradient(120, 120);

        let run = |noise: f32| {
            let mut vars = vars_for(Mode::Photo);
            vars.noise = noise;
            vars.denoise = su_core::DenoiseChoice::On;
            run_with(&image, &photo_pipeline(), &vars, &RunnerConfig::default()).image
        };

        let moderate = run(0.40);
        let heavy = run(0.95);

        assert_eq!(moderate.width, heavy.width);
        assert_eq!(moderate.rgb.len(), heavy.rgb.len());

        let difference: f32 = moderate
            .rgb
            .iter()
            .zip(heavy.rgb.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();

        assert!(
            difference > 0.0,
            "el peso del denoise no depende del ruido medido: los dos resultados son iguales"
        );
    }

    #[test]
    fn a_face_stage_without_faces_says_so() {
        // Una cadena propia puede declarar `onlyOnFaces` sin condicion. En ese caso la
        // etapa no se ejecuta, y el motivo tiene que decirlo con esas palabras: es lo
        // que el usuario ve al arrastrar un paisaje en modo Foto.
        let image = gradient(64, 64);
        let pipeline = su_core::Pipeline {
            id: "test:caras".to_string(),
            mode: Mode::Photo,
            scale: Scale::X4,
            description: String::new(),
            stages: vec![
                model_stage("upscale", "4x-ultrasharp", 4),
                su_core::Stage {
                    only_on_faces: true,
                    ..model_stage("caras", "gfpgan-v1.4", 1)
                },
            ],
        };

        let outcome = run_with(
            &image,
            &pipeline,
            &vars_for(Mode::Photo),
            &RunnerConfig::default(),
        );

        assert!(!outcome.executed.contains(&StageId::Face));
        assert!(
            outcome
                .skipped
                .iter()
                .any(|(_, reason)| reason == "no se detectaron caras"),
            "motivos: {:?}",
            outcome.skipped
        );
    }

    #[test]
    fn stages_that_are_not_implemented_are_reported_as_skipped() {
        // Regla del proyecto: nada se omite en silencio.
        let image = gradient(80, 80);
        let mut vars = vars_for(Mode::Photo);
        vars.denoise = su_core::DenoiseChoice::On;
        vars.noise = 0.9;

        // El pipeline de ilustracion usa una etapa de limpieza de lineas que si
        // esta implementada; para probar el reporte de omitidas se usa una etapa
        // clasica que aun no lo esta.
        let pipeline = su_core::Pipeline {
            id: "test:skipped".to_string(),
            mode: Mode::Photo,
            scale: Scale::X4,
            description: String::new(),
            stages: vec![
                su_core::Stage {
                    id: "clasico".to_string(),
                    op: su_core::StageOp::DenoiseClassic,
                    model: None,
                    fallback_model: None,
                    when: None,
                    tiling: None,
                    scale_out: None,
                    blend: None,
                    blend_from: None,
                    only_on_faces: false,
                    overlap_boost: 0,
                    kernel: None,
                    factor: None,
                    amount: None,
                    radius: None,
                    threshold: None,
                },
                su_core::Stage {
                    id: "upscale".to_string(),
                    op: su_core::StageOp::Model,
                    model: Some("4x-ultrasharp".to_string()),
                    fallback_model: None,
                    when: None,
                    tiling: None,
                    scale_out: Some(4),
                    blend: None,
                    blend_from: None,
                    only_on_faces: false,
                    overlap_boost: 0,
                    kernel: None,
                    factor: None,
                    amount: None,
                    radius: None,
                    threshold: None,
                },
            ],
        };

        let outcome = run_with(&image, &pipeline, &vars, &RunnerConfig::default());

        assert!(outcome.executed.contains(&StageId::Upscale));
        assert_eq!(outcome.skipped.len(), 1);
        assert!(outcome.skipped[0].1.contains("Fase 3"), "{:?}", outcome.skipped);
    }

    #[test]
    fn a_model_stage_without_a_model_is_skipped_not_silently_ignored() {
        let image = gradient(64, 64);
        let mut huerfana = model_stage("huerfana", "4x-ultrasharp", 4);
        huerfana.model = None;
        huerfana.scale_out = None;

        let pipeline = su_core::Pipeline {
            id: "test:sin-modelo".to_string(),
            mode: Mode::Photo,
            scale: Scale::X4,
            description: String::new(),
            stages: vec![
                huerfana,
                // El resto del pipeline tiene que seguir llegando a x4: si no, el
                // fallo seria otro (el pipeline no cumple su escala) y este test
                // dejaria de comprobar lo que dice comprobar.
                model_stage("upscale", "4x-ultrasharp", 4),
            ],
        };

        let outcome = run_with(&image, &pipeline, &vars_for(Mode::Photo), &RunnerConfig::default());

        assert_eq!(outcome.skipped.len(), 1, "{:?}", outcome.skipped);
        assert!(
            outcome.skipped[0].1.contains("no declara modelo"),
            "{:?}",
            outcome.skipped
        );
        assert!(outcome.executed.contains(&StageId::Upscale));
        assert_eq!(outcome.image.width, 256);
    }

    #[test]
    fn a_stage_whose_condition_fails_is_reported_as_skipped() {
        // `docs/04` promete que una etapa que no entra por su condicion "se
        // omite y se emite un aviso claro". Antes el runner hacia un `continue`
        // mudo: la imagen salia distinta y el resumen no decia por que.
        let image = gradient(64, 64);

        let unsharp = |id: &str, when: Option<su_core::Condition>| su_core::Stage {
            id: id.to_string(),
            op: su_core::StageOp::Unsharp,
            model: None,
            fallback_model: None,
            when,
            tiling: None,
            scale_out: None,
            blend: None,
            blend_from: None,
            only_on_faces: false,
            overlap_boost: 0,
            kernel: None,
            factor: None,
            amount: None,
            radius: None,
            threshold: None,
        };

        let pipeline = su_core::Pipeline {
            id: "test:condicional".to_string(),
            mode: Mode::Photo,
            scale: Scale::X4,
            description: String::new(),
            stages: vec![
                // El pipeline tiene que llegar a x4: una etapa condicional omitida
                // no puede cambiar el tamano del resultado sin decirlo.
                model_stage("upscale", "4x-ultrasharp", 4),
                unsharp(
                    "enfoque",
                    Some(su_core::Condition::Truthy {
                        path: "prefs.sharpen".to_string(),
                        negated: false,
                    }),
                ),
                // Segunda etapa sin condicion, para que el pipeline haga algo y
                // se pueda distinguir "omitida" de "no habia nada que hacer".
                unsharp("enfoque-final", None),
            ],
        };

        let apagado = run_with(&image, &pipeline, &vars_for(Mode::Photo), &RunnerConfig::default());
        assert!(apagado.executed.contains(&StageId::Sharpen));
        assert_eq!(apagado.skipped.len(), 1, "{:?}", apagado.skipped);
        assert_eq!(apagado.skipped[0].0, StageId::Sharpen);
        assert!(
            apagado.skipped[0].1.contains("enfoque"),
            "el motivo tiene que nombrar la etapa: {:?}",
            apagado.skipped
        );

        // Con el enfoque encendido la etapa entra y no hay nada que reportar.
        let mut vars = vars_for(Mode::Photo);
        vars.sharpen = true;
        let encendido = run_with(&image, &pipeline, &vars, &RunnerConfig::default());
        assert!(encendido.skipped.is_empty(), "{:?}", encendido.skipped);
    }

    #[test]
    fn an_eight_x_that_does_not_fit_is_refused_before_starting() {
        // El limite de megapixeles no se aplica omitiendo `upscale2` a mitad de la
        // cadena: eso devolvia 2x cuando el usuario habia pedido 8x, y encima
        // despues de haber hecho todo el trabajo. Se rechaza antes de empezar, con
        // el motivo, para que el usuario pueda bajar la escala.
        let image = gradient(64, 64);
        let pipeline = PipelineSet::embedded()
            .expect("pipelines embebidos")
            .get(Mode::Photo, Scale::X8)
            .expect("pipeline photo:8x")
            .clone();

        let mut vars = vars_for(Mode::Photo);
        vars.estimated_output_mp = 4_000.0;

        let error = run_err(&image, &pipeline, &vars);

        assert_eq!(error.code().as_str(), "SU-E142");
        assert!(error.to_string().contains("4000 MP"), "{error}");
        assert!(error.to_string().contains("800 MP"), "{error}");
    }

    #[test]
    fn the_eight_x_pipeline_reaches_eight_x_when_it_fits() {
        // La otra mitad del contrato. Antes `upscale2` se omitia por encima de 800
        // MP aunque el limite configurado fuese otro, asi que el resultado dependia
        // de un numero escrito a mano en el JSON en lugar de la preferencia del
        // usuario. Ahora el unico guardian es el de arriba.
        let image = gradient(64, 64);
        let pipeline = PipelineSet::embedded()
            .expect("pipelines embebidos")
            .get(Mode::Photo, Scale::X8)
            .expect("pipeline photo:8x")
            .clone();

        let mut vars = vars_for(Mode::Photo);
        vars.estimated_output_mp = 799.0;

        let outcome = run_with(&image, &pipeline, &vars, &RunnerConfig::default());

        assert_eq!(outcome.image.width, 512);
        assert!(
            !outcome
                .skipped
                .iter()
                .any(|(_, reason)| reason.contains("upscale2")),
            "upscale2 no puede omitirse: {:?}",
            outcome.skipped
        );
    }

    #[test]
    fn a_restoration_stage_that_uses_a_4x_model_stays_at_scale_one() {
        // Es el caso de `lineclean`: un modelo x4 que limpia, no que amplia. El
        // runner tiene que reducir la salida a la escala que declara la etapa. Sin
        // eso, la cadena sale multiplicada y `illustration:4x` produce 16x: el
        // usuario pide 4x y recibe cuatro veces mas pixeles de los que pidio.
        let image = gradient(64, 64);
        let mut lineclean = model_stage("lineclean", "realesrgan-x4plus-anime-6b", 1);
        lineclean.blend = Some(0.7);

        let pipeline = su_core::Pipeline {
            id: "test:restauracion".to_string(),
            mode: Mode::Illustration,
            scale: Scale::X4,
            description: String::new(),
            stages: vec![
                lineclean,
                model_stage("upscale", "realesrgan-x4plus-anime-6b", 4),
            ],
        };

        let outcome = run_with(
            &image,
            &pipeline,
            &vars_for(Mode::Illustration),
            &RunnerConfig::default(),
        );

        assert_eq!(outcome.image.width, 256, "x4, no x16");
        assert_eq!(outcome.image.height, 256);
        assert!(
            outcome.executed.contains(&StageId::Denoise),
            "una limpieza se reporta como limpieza, no como escalado: {:?}",
            outcome.executed
        );
    }

    #[test]
    fn a_stage_that_asks_for_more_scale_than_the_model_gives_is_an_error() {
        // Pedir x4 a un modelo x2 no tiene arreglo: no hay forma de inventar el
        // detalle que falta. Es mejor fallar con un mensaje claro que devolver una
        // imagen del tamano equivocado.
        let image = gradient(64, 64);
        let pipeline = su_core::Pipeline {
            id: "test:escala".to_string(),
            mode: Mode::Photo,
            scale: Scale::X4,
            description: String::new(),
            stages: vec![
                // El mock deduce x2 del identificador, pero la etapa pide x4.
                model_stage("upscale", "2x-animesharpv3", 4),
            ],
        };

        let error = run_err(&image, &pipeline, &vars_for(Mode::Photo));

        assert!(error.to_string().contains("2x-animesharpv3"), "{error}");
        assert!(error.to_string().contains("x2"), "{error}");
    }

    #[test]
    fn a_stage_that_asks_for_less_scale_than_the_model_gives_reduces_instead() {
        // Lo contrario si tiene sentido, y es lo que hace `photo:2x` con su etapa
        // de reduccion: escalar a 4x y bajar a 2x conserva mas detalle que recortar
        // la salida. Que el modelo de mas no es un error; que de menos, si.
        let image = gradient(64, 64);
        let pipeline = su_core::Pipeline {
            id: "test:escala".to_string(),
            mode: Mode::Photo,
            scale: Scale::X2,
            description: String::new(),
            stages: vec![model_stage("upscale", "4x-ultrasharp", 2)],
        };

        let outcome = run_with(&image, &pipeline, &vars_for(Mode::Photo), &RunnerConfig::default());

        assert_eq!(outcome.image.width, 128);
        assert_eq!(outcome.image.height, 128);
    }

    /// Proveedor que apunta los modelos que se le piden. Es la unica forma de
    /// comprobar a que etapas llega el modelo elegido a mano sin abrir un modelo
    /// de verdad.
    #[derive(Default)]
    struct RecordingProvider {
        asked: std::sync::Mutex<Vec<String>>,
    }

    impl RecordingProvider {
        fn asked(&self) -> Vec<String> {
            self.asked.lock().expect("lock envenenado").clone()
        }
    }

    impl BackendProvider for RecordingProvider {
        fn backend_for(&self, model_id: &str) -> su_core::SuResult<Box<dyn Backend>> {
            self.asked
                .lock()
                .expect("lock envenenado")
                .push(model_id.to_string());
            Ok(Box::new(
                MockBackend::new(MockBackendProvider::scale_for(model_id))?.with_id(model_id),
            ))
        }

        fn native_scale(&self, model_id: &str) -> Option<u32> {
            Some(MockBackendProvider::scale_for(model_id))
        }
    }

    /// Proveedor que no puede cargar los modelos que se le indican.
    ///
    /// Es la unica forma de comprobar que pasa cuando falta un modelo sin
    /// descargar cientos de megabytes ni depender de lo que haya instalado en la
    /// maquina donde se ejecutan las pruebas.
    struct MissingModelsProvider {
        missing: Vec<&'static str>,
    }

    impl BackendProvider for MissingModelsProvider {
        fn backend_for(&self, model_id: &str) -> su_core::SuResult<Box<dyn Backend>> {
            if self.missing.contains(&model_id) {
                return Err(SuError::ModelMissing(model_id.to_string()));
            }
            Ok(Box::new(
                MockBackend::new(MockBackendProvider::scale_for(model_id))?.with_id(model_id),
            ))
        }

        fn native_scale(&self, model_id: &str) -> Option<u32> {
            Some(MockBackendProvider::scale_for(model_id))
        }
    }

    #[test]
    fn a_missing_scaling_model_fails_with_the_model_that_is_missing() {
        // La etapa que escala no se puede omitir: el resultado tendria otra
        // medida. El error tiene que nombrar el modelo que falta.
        let image = gradient(64, 64);
        let provider = MissingModelsProvider {
            missing: vec!["4x-ultrasharp", "realesrgan-x4plus"],
        };

        let error = run_pipeline(
            &image,
            &photo_pipeline(),
            &vars_for(Mode::Photo),
            &provider,
            &RunnerConfig::default(),
            &mut |_| {},
        )
        .expect_err("sin modelo de escalado el pipeline no puede terminar");

        assert_eq!(error.code().as_str(), "SU-E110");
        assert!(error.to_string().contains("4x-ultrasharp"), "{error}");
    }

    #[test]
    fn a_missing_restoration_model_is_skipped_with_its_reason() {
        // El caso real que esto arregla: el modelo de limpieza de lineas no esta
        // instalado (y no se puede descargar: su export reparte los pesos en dos
        // archivos). Antes la imagen entera fallaba con "modelo no disponible"
        // aunque el modelo que escala si estuviera listo.
        let image = gradient(64, 64);
        let mut lineclean = model_stage("lineclean", "realesrgan-x4plus-anime-6b", 1);
        lineclean.blend = Some(0.7);

        let pipeline = su_core::Pipeline {
            id: "test:restauracion-ausente".to_string(),
            mode: Mode::Illustration,
            scale: Scale::X4,
            description: String::new(),
            stages: vec![lineclean, model_stage("upscale", "4x-ultrasharp", 4)],
        };

        let mut vars = vars_for(Mode::Illustration);
        vars.noise = 0.9; // activa la limpieza

        let provider = MissingModelsProvider {
            missing: vec!["realesrgan-x4plus-anime-6b"],
        };

        let outcome = run_pipeline(
            &image,
            &pipeline,
            &vars,
            &provider,
            &RunnerConfig::default(),
            &mut |_| {},
        )
        .expect("la etapa de limpieza es opcional");

        assert_eq!(outcome.image.width, 256, "la escala prometida se mantiene");
        assert_eq!(outcome.skipped.len(), 1, "{:?}", outcome.skipped);
        assert!(
            outcome.skipped[0].1.contains("realesrgan-x4plus-anime-6b"),
            "el motivo tiene que nombrar el modelo ausente: {:?}",
            outcome.skipped
        );
    }

    /// Imagen con silueta: la mitad izquierda opaca y la derecha transparente.
    ///
    /// Sin suavizado a proposito: lo que se quiere poder ver es el bloque que
    /// devuelve el backend simulado, que es vecino mas cercano.
    fn with_silhouette(width: u32, height: u32) -> DecodedImage {
        let mut image = gradient(width, height);
        let mut alpha = Vec::with_capacity((width * height) as usize);
        for _ in 0..height {
            for x in 0..width {
                alpha.push(if x < width / 2 { 1.0 } else { 0.0 });
            }
        }
        image.alpha = Some(alpha);
        image
    }

    /// Proveedor que cuenta cuantas veces se **ejecuta** de verdad cada modelo.
    ///
    /// Hace falta porque [`RecordingProvider`] cuenta las veces que se *pide* un
    /// modelo, y eso no distingue una etapa que corre dos veces —el color y la
    /// silueta— de una que corre una: el backend se pide una sola vez por etapa y
    /// se reutiliza para las dos pasadas. Sin este contador, la unica forma de
    /// comprobar que la silueta pasa por el modelo seria mirar los pixeles, y con
    /// un backend simulado eso confunde geometria con calidad.
    #[derive(Debug, Default)]
    struct CountingProvider {
        runs: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, u32>>>,
    }

    impl CountingProvider {
        fn runs(&self, model_id: &str) -> u32 {
            self.runs
                .lock()
                .expect("lock envenenado")
                .get(model_id)
                .copied()
                .unwrap_or(0)
        }
    }

    #[derive(Debug)]
    struct CountingBackend {
        inner: MockBackend,
        model_id: String,
        runs: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, u32>>>,
    }

    impl Backend for CountingBackend {
        fn id(&self) -> &str {
            &self.model_id
        }

        fn scale(&self) -> u32 {
            self.inner.scale()
        }

        fn supports_full_image(&self) -> bool {
            self.inner.supports_full_image()
        }

        fn run_tile(&mut self, input: &TileInput<'_>) -> su_core::SuResult<TileOutput> {
            *self
                .runs
                .lock()
                .expect("lock envenenado")
                .entry(self.model_id.clone())
                .or_insert(0) += 1;
            self.inner.run_tile(input)
        }
    }

    impl BackendProvider for CountingProvider {
        fn backend_for(&self, model_id: &str) -> su_core::SuResult<Box<dyn Backend>> {
            Ok(Box::new(CountingBackend {
                inner: MockBackend::new(MockBackendProvider::scale_for(model_id))?
                    .with_id(model_id),
                model_id: model_id.to_string(),
                runs: self.runs.clone(),
            }))
        }

        fn native_scale(&self, model_id: &str) -> Option<u32> {
            Some(MockBackendProvider::scale_for(model_id))
        }
    }

    /// Pipeline minimo: una sola etapa que escala.
    fn single_upscale_pipeline(model: &str) -> su_core::Pipeline {
        su_core::Pipeline {
            id: "test:silueta".to_string(),
            mode: Mode::Illustration,
            scale: Scale::X4,
            description: String::new(),
            stages: vec![model_stage("upscale", model, 4)],
        }
    }

    #[test]
    fn the_silhouette_is_scaled_by_the_model_and_not_by_an_interpolator() {
        // El defecto que esto arregla: el color lo reconstruia el modelo con un
        // borde de 1 px mientras el alfa se interpolaba aparte con Lanczos y salia
        // con una rampa de 4 a 7 px. El resultado se ve como un halo blando
        // alrededor de todo el contorno, por muy nitido que este el interior.
        // Medido sobre un dibujo con transparencia, la rampa del alfa pasa de 4-7
        // px a 0-1 px, que es lo que da la referencia.
        let pipeline = single_upscale_pipeline("4x-anime");
        let vars = vars_for(Mode::Illustration);
        let config = RunnerConfig::default();

        let provider = CountingProvider::default();
        let con_silueta = run_pipeline(
            &with_silhouette(32, 32),
            &pipeline,
            &vars,
            &provider,
            &config,
            &mut |_| {},
        )
        .expect("ejecucion");

        let provider_sin = CountingProvider::default();
        let sin_silueta = run_pipeline(
            &gradient(32, 32),
            &pipeline,
            &vars,
            &provider_sin,
            &config,
            &mut |_| {},
        )
        .expect("ejecucion");

        assert_eq!(sin_silueta.image.alpha, None, "sin alfa no hay silueta que escalar");
        assert_eq!(
            provider_sin.runs("4x-anime") * 2,
            provider.runs("4x-anime"),
            "la etapa que escala tiene que ejecutar el modelo dos veces: el color y la silueta"
        );

        let alpha = con_silueta
            .image
            .alpha
            .clone()
            .expect("la silueta tiene que llegar a la imagen");
        assert_eq!(alpha.len(), 128 * 128, "el alfa tiene el tamano de la salida");

        // Los bloques de 4x4 son la firma del backend simulado: si la silueta se
        // hubiera interpolado aparte, el alfa seria una rampa y esto no se cumpliria.
        for y in 0..128usize {
            for x in 0..128usize {
                let origen = (y / 4 * 4) * 128 + (x / 4 * 4);
                assert_eq!(
                    alpha[y * 128 + x],
                    alpha[origen],
                    "el alfa de ({x},{y}) no es el de su bloque de 4x4"
                );
            }
        }
    }

    #[test]
    fn a_restoration_stage_does_not_grind_the_silhouette() {
        // `lineclean` usa un modelo x4 y declara `scaleOut: 1`: sube a x4 y vuelve a
        // bajar. La silueta no debe pasar por ahi. Cuesta una inferencia de mas y,
        // sobre todo, el reescalado de vuelta volveria a difuminar el contorno que
        // se acaba de reconstruir.
        let mut limpieza = model_stage("lineclean", "4x-limpieza", 1);
        limpieza.blend = Some(0.7);

        let pipeline = su_core::Pipeline {
            id: "test:limpieza".to_string(),
            mode: Mode::Illustration,
            scale: Scale::X4,
            description: String::new(),
            stages: vec![limpieza, model_stage("upscale", "4x-escala", 4)],
        };
        let vars = vars_for(Mode::Illustration);
        let config = RunnerConfig::default();

        let provider = CountingProvider::default();
        let outcome = run_pipeline(
            &with_silhouette(32, 32),
            &pipeline,
            &vars,
            &provider,
            &config,
            &mut |_| {},
        )
        .expect("ejecucion");

        let provider_sin = CountingProvider::default();
        let _sin_silueta = run_pipeline(
            &gradient(32, 32),
            &pipeline,
            &vars,
            &provider_sin,
            &config,
            &mut |_| {},
        )
        .expect("ejecucion");

        assert_eq!(
            provider.runs("4x-limpieza"),
            provider_sin.runs("4x-limpieza"),
            "la limpieza no cambia el tamano: la silueta no tiene que pasar por ella"
        );
        assert_eq!(
            provider_sin.runs("4x-escala") * 2,
            provider.runs("4x-escala"),
            "la etapa que escala si lleva la silueta"
        );

        assert_eq!(outcome.image.width, 128);
        assert_eq!(outcome.image.height, 128);
        assert_eq!(
            outcome.image.alpha.as_ref().map(Vec::len),
            Some(128 * 128),
            "la silueta sigue teniendo el tamano de la imagen"
        );
    }

    #[test]
    fn the_reserve_model_is_used_when_the_preferred_one_is_not_installed() {
        // `fallbackModel` existia en los seis pipelines embebidos y no se leia.
        let image = gradient(64, 64);
        let provider = MissingModelsProvider {
            missing: vec!["4x-ultrasharp"],
        };

        let outcome = run_pipeline(
            &image,
            &photo_pipeline(),
            &vars_for(Mode::Photo),
            &provider,
            &RunnerConfig::default(),
            &mut |_| {},
        )
        .expect("el modelo de reserva deberia cubrir la etapa");

        assert_eq!(outcome.image.width, 256);
        assert!(outcome.executed.contains(&StageId::Upscale));
        assert!(
            outcome
                .skipped
                .iter()
                .any(|(_, reason)| reason.contains("modelo de reserva")
                    && reason.contains("realesrgan-x4plus")),
            "tiene que quedar dicho que se uso el de reserva: {:?}",
            outcome.skipped
        );
    }

    #[test]
    fn a_manual_model_replaces_the_one_the_pipeline_hardcodes() {
        // Modo Manual: el usuario elige un modelo y espera que se use. Antes la
        // eleccion se validaba en la interfaz, se guardaba en los ajustes y no
        // llegaba nunca al motor, asi que el selector no cambiaba nada.
        let image = gradient(64, 64);
        let pipeline = photo_pipeline();
        let mut vars = vars_for(Mode::Photo);
        vars.model_chain_mode = su_core::ModelChainMode::Manual;

        let config = RunnerConfig {
            model_override: Some("realesrgan-x4plus".to_string()),
            ..RunnerConfig::default()
        };

        let provider = RecordingProvider::default();
        let outcome = run_pipeline(&image, &pipeline, &vars, &provider, &config, &mut |_| {})
            .expect("ejecucion");

        // El override es otro modelo x4, asi que el tamano es el mismo que sin el:
        // 64 x 4. La comprobacion de que el override llega al motor son las dos
        // siguientes, no esta.
        assert_eq!(outcome.image.width, 256, "64 a x4 son 256 columnas");
        assert_eq!(outcome.image.height, 256);

        let asked = provider.asked();
        assert!(
            asked.iter().any(|model| model == "realesrgan-x4plus"),
            "el modelo elegido tiene que llegar al motor: {asked:?}"
        );
        assert!(
            !asked.iter().any(|model| model == "4x-ultrasharp"),
            "el modelo del pipeline no deberia usarse: {asked:?}"
        );
    }

    #[test]
    fn a_manual_model_does_not_replace_the_restoration_model() {
        // `lineclean` limpia, no escala. Sustituirlo por un modelo que escala
        // cambiaria el tamano de la imagen, que es justo lo que el override no debe
        // hacer: la eleccion del usuario es sobre el modelo que amplia.
        let image = gradient(64, 64);
        let mut lineclean = model_stage("lineclean", "realesrgan-x4plus-anime-6b", 1);
        lineclean.blend = Some(0.7);

        let pipeline = su_core::Pipeline {
            id: "test:restauracion".to_string(),
            mode: Mode::Illustration,
            scale: Scale::X4,
            description: String::new(),
            stages: vec![
                lineclean,
                model_stage("upscale", "realesrgan-x4plus-anime-6b", 4),
            ],
        };

        // El ruido alto activa la etapa de limpieza.
        let mut vars = vars_for(Mode::Illustration);
        vars.noise = 0.5;

        let config = RunnerConfig {
            model_override: Some("4x-ultrasharp".to_string()),
            ..RunnerConfig::default()
        };

        let provider = RecordingProvider::default();
        let outcome = run_pipeline(&image, &pipeline, &vars, &provider, &config, &mut |_| {})
            .expect("ejecucion");

        assert_eq!(outcome.image.width, 256, "x4, no x16");
        let asked = provider.asked();
        assert!(
            asked.iter().any(|model| model == "realesrgan-x4plus-anime-6b"),
            "la limpieza conserva su modelo: {asked:?}"
        );
        assert!(
            asked.iter().any(|model| model == "4x-ultrasharp"),
            "el escalado usa el elegido: {asked:?}"
        );
    }

    #[test]
    fn a_manual_model_that_cannot_reach_the_scale_is_an_error() {
        // Elegir un modelo x2 para un pipeline x4 no puede quedarse en silencio.
        let image = gradient(64, 64);
        let pipeline = photo_pipeline();

        let config = RunnerConfig {
            model_override: Some("2x-animesharpv3".to_string()),
            ..RunnerConfig::default()
        };

        let provider = MockBackendProvider;
        let error = run_pipeline(
            &image,
            &pipeline,
            &vars_for(Mode::Photo),
            &provider,
            &config,
            &mut |_| {},
        )
        .expect_err("un modelo x2 no puede dar x4");

        assert!(error.to_string().contains("2x-animesharpv3"), "{error}");
    }

    #[test]
    fn the_two_x_pipeline_lands_on_the_right_size() {
        let image = gradient(160, 100);
        let pipeline = PipelineSet::embedded()
            .unwrap()
            .get(Mode::Photo, Scale::X2)
            .unwrap()
            .clone();

        let outcome = run_with(&image, &pipeline, &vars_for(Mode::Photo), &RunnerConfig::default());

        // La cadena es x4 y despues reduccion a la mitad.
        assert_eq!(outcome.image.width, 320);
        assert_eq!(outcome.image.height, 200);
        assert!(outcome.executed.contains(&StageId::Upscale));
    }

    #[test]
    fn the_eight_x_pipeline_lands_on_the_right_size() {
        let image = gradient(64, 64);
        let pipeline = PipelineSet::embedded()
            .unwrap()
            .get(Mode::Photo, Scale::X8)
            .unwrap()
            .clone();

        let outcome = run_with(&image, &pipeline, &vars_for(Mode::Photo), &RunnerConfig::default());

        assert_eq!(outcome.image.width, 512);
        assert_eq!(outcome.image.height, 512);
    }

    #[test]
    fn the_illustration_pipeline_has_no_face_stage() {
        let image = gradient(100, 100);
        let pipeline = PipelineSet::embedded()
            .unwrap()
            .get(Mode::Illustration, Scale::X4)
            .unwrap()
            .clone();

        let mut vars = vars_for(Mode::Illustration);
        vars.face_count = 5;
        vars.face_restore = su_core::FaceRestoreChoice::High;

        let outcome = run_with(&image, &pipeline, &vars, &RunnerConfig::default());
        assert!(!outcome.executed.contains(&StageId::Face));
    }

    #[test]
    fn progress_is_reported_per_tile() {
        let image = gradient(600, 400);
        let config = RunnerConfig {
            tile_choice: TileChoice::Px256,
            ..RunnerConfig::default()
        };
        let pipeline = photo_pipeline();
        let vars = vars_for(Mode::Photo);
        let provider = MockBackendProvider;

        let mut updates: Vec<StageProgress> = Vec::new();
        run_pipeline(&image, &pipeline, &vars, &provider, &config, &mut |progress| {
            updates.push(progress)
        })
        .expect("ejecucion");

        let upscale: Vec<_> = updates
            .iter()
            .filter(|update| update.stage == StageId::Upscale)
            .collect();

        assert!(!upscale.is_empty(), "no se reporto progreso de escalado");
        assert!(upscale.iter().all(|update| update.total > 1), "deberia haber varios tiles");
        assert_eq!(
            upscale.last().map(|update| update.done),
            upscale.first().map(|update| update.total)
        );
    }

    // -----------------------------------------------------------------------
    // Degradacion progresiva
    // -----------------------------------------------------------------------

    /// Backend que simula falta de memoria por encima de un tamano de tile.
    #[derive(Debug)]
    struct MemoryLimitedBackend {
        inner: MockBackend,
        max_tile: u32,
        failures: u32,
    }

    impl Backend for MemoryLimitedBackend {
        fn id(&self) -> &str {
            "memory-limited"
        }

        fn scale(&self) -> u32 {
            self.inner.scale()
        }

        fn run_tile(&mut self, input: &TileInput<'_>) -> su_core::SuResult<TileOutput> {
            if input.width > self.max_tile || input.height > self.max_tile {
                self.failures += 1;
                return Err(SuError::OutOfVram {
                    tile: input.width.max(input.height),
                    free_mb: 1024,
                    needed_mb: 4096,
                });
            }
            self.inner.run_tile(input)
        }
    }

    struct LimitedProvider {
        max_tile: u32,
    }

    impl BackendProvider for LimitedProvider {
        fn backend_for(&self, model_id: &str) -> su_core::SuResult<Box<dyn Backend>> {
            Ok(Box::new(MemoryLimitedBackend {
                inner: MockBackend::new(MockBackendProvider::scale_for(model_id))?,
                max_tile: self.max_tile,
                failures: 0,
            }))
        }
    }

    #[test]
    fn running_out_of_memory_degrades_the_tile_instead_of_failing() {
        let image = gradient(1024, 1024);
        let pipeline = photo_pipeline();
        let vars = vars_for(Mode::Photo);

        // El backend solo admite tiles de 256 o menos.
        let provider = LimitedProvider { max_tile: 256 };
        let config = RunnerConfig {
            tile_choice: TileChoice::Px1024,
            candidates: vec![1024, 768, 512, 384, 256, 192],
            ..RunnerConfig::default()
        };

        let outcome = run_pipeline(&image, &pipeline, &vars, &provider, &config, &mut |_| {})
            .expect("deberia degradar en lugar de fallar");

        assert!(outcome.degradations >= 2, "degradaciones: {}", outcome.degradations);
        assert_eq!(outcome.final_tile, Some(256));
        // Y el resultado sigue siendo correcto.
        assert_eq!(outcome.image.width, 4096);
        assert!(su_imageio::validate_output(&outcome.image).is_ok());
    }

    #[test]
    fn if_no_tile_works_the_error_surfaces() {
        let image = gradient(512, 512);
        let pipeline = photo_pipeline();
        let vars = vars_for(Mode::Photo);

        // Ni siquiera el minimo cabe.
        let provider = LimitedProvider { max_tile: 8 };
        let config = RunnerConfig {
            tile_choice: TileChoice::Px256,
            candidates: vec![256, 192],
            max_degradation_steps: 2,
            ..RunnerConfig::default()
        };

        let result = run_pipeline(&image, &pipeline, &vars, &provider, &config, &mut |_| {});
        assert!(result.is_err(), "deberia fallar cuando ningun tile cabe");
    }

    #[test]
    fn a_budget_chooses_the_initial_tile() {
        let image = gradient(2048, 2048);
        let pipeline = photo_pipeline();
        let vars = vars_for(Mode::Photo);

        let config = RunnerConfig {
            tile_choice: TileChoice::Auto,
            budget: Some(VramBudget::for_provider(
                4096,
                70,
                2500.0,
                su_tiling::ProviderKind::TensorRt,
            )),
            ..RunnerConfig::default()
        };

        let outcome = run_with(&image, &pipeline, &vars, &config);
        // Un modelo pesado en una GPU de 4 GB no puede usar el tile maximo.
        assert!(outcome.final_tile.unwrap_or(0) < 1024);
        assert_eq!(outcome.degradations, 0, "no deberia hacer falta degradar");
    }

    #[test]
    fn without_vram_data_a_large_image_uses_the_biggest_tile() {
        // Esta prueba se llamaba "se trocea con prudencia" y no comprobaba el tile:
        // pasaba con cualquier valor. Lo que decia el comentario del codigo era
        // "el menor de los que sean razonables", y lo que hacia era coger 256, asi
        // que una imagen de 1500x1500 se partia en 64 tiles en CPU en lugar de 4.
        let image = gradient(1500, 1500);
        let pipeline = photo_pipeline();
        let vars = vars_for(Mode::Photo);

        let outcome = run_with(&image, &pipeline, &vars, &RunnerConfig::default());
        assert_eq!(outcome.image.width, 6000);
        assert_eq!(outcome.degradations, 0);
        assert_eq!(
            outcome.final_tile,
            Some(1024),
            "sin datos de VRAM manda el candidato mayor: es el que menos veces paga \
             el coste fijo de una inferencia y el solape"
        );
    }

    #[test]
    fn the_tile_without_a_budget_is_the_smallest_that_covers_the_image_or_the_biggest() {
        let candidatos = [1024, 768, 512, 384, 256, 192];

        // La imagen cabe: el menor de los que la cubren, para no trocear de mas.
        assert_eq!(choose_tile_without_budget(192, &candidatos), 192);
        assert_eq!(choose_tile_without_budget(300, &candidatos), 384);
        assert_eq!(choose_tile_without_budget(512, &candidatos), 512);
        assert_eq!(choose_tile_without_budget(768, &candidatos), 768);
        assert_eq!(choose_tile_without_budget(1024, &candidatos), 1024);

        // No cabe: el mayor. Nunca el menor, que es lo que hacia antes.
        assert_eq!(choose_tile_without_budget(1025, &candidatos), 1024);
        assert_eq!(choose_tile_without_budget(3000, &candidatos), 1024);
        assert_eq!(choose_tile_without_budget(8000, &candidatos), 1024);

        // Lista desordenada: la respuesta no puede depender del orden.
        assert_eq!(
            choose_tile_without_budget(700, &[192, 1024, 256, 768, 384, 512]),
            768
        );

        // Cada vez que la imagen es mas grande, el tile no puede ser menor.
        let mut anterior = 0;
        for lado in [128, 256, 384, 512, 768, 1024, 2048, 4096] {
            let elegido = choose_tile_without_budget(lado, &candidatos);
            assert!(
                elegido >= anterior,
                "el tile bajo de {anterior} a {elegido} al crecer la imagen a {lado}"
            );
            anterior = elegido;
        }

        // Casos degenerados: sin candidatos utiles se cae al minimo.
        assert_eq!(choose_tile_without_budget(3000, &[]), su_tiling::MIN_TILE);
        assert_eq!(
            choose_tile_without_budget(3000, &[32, 16]),
            su_tiling::MIN_TILE,
            "un candidato por debajo del minimo no puede usarse"
        );
        assert_eq!(choose_tile_without_budget(3000, &[64, 128]), 128);
    }

    /// Proveedor que declara las pistas de tiling de su modelo, como hace el real
    /// leyendo el manifiesto.
    struct HintsProvider {
        hints: TilingHints,
    }

    impl BackendProvider for HintsProvider {
        fn backend_for(&self, model_id: &str) -> su_core::SuResult<Box<dyn Backend>> {
            Ok(Box::new(
                MockBackend::new(MockBackendProvider::scale_for(model_id))?.with_id(model_id),
            ))
        }

        fn native_scale(&self, model_id: &str) -> Option<u32> {
            Some(MockBackendProvider::scale_for(model_id))
        }

        fn tiling_hints(&self, _model_id: &str) -> Option<TilingHints> {
            Some(self.hints.clone())
        }
    }

    fn hints(candidates: &[u32], overlap_divisor: u32, pad_to: u32, per_mp: Option<f32>) -> TilingHints {
        TilingHints {
            candidates: candidates.to_vec(),
            overlap_divisor,
            pad_to,
            vram_per_megapixel: per_mp,
        }
    }

    #[test]
    fn the_manifest_tile_candidates_reach_the_runner() {
        // La seccion `tiling` del manifiesto se parseaba, se validaba y se probaba, y
        // no llegaba nunca al runner: todos los modelos corrian con los candidatos por
        // defecto. Un modelo que declara [64, 128] porque el tile grande le hace
        // producir artefactos recibia 1024.
        let image = gradient(256, 256);
        let pipeline = single_upscale_pipeline("4x-anime");
        let vars = vars_for(Mode::Illustration);
        let config = RunnerConfig::default();

        let provider = MockBackendProvider;
        let sin_pistas = run_pipeline(
            &image,
            &pipeline,
            &vars,
            &provider,
            &config,
            &mut |_| {},
        )
        .expect("ejecucion");
        assert_eq!(sin_pistas.final_tile, Some(256), "256 cabe en el candidato 256");

        // La imagen de 256 px no cabe en ninguno de los dos: manda el mayor.
        let provider = HintsProvider {
            hints: hints(&[64, 128], 16, 32, None),
        };
        let con_pistas = run_pipeline(
            &image,
            &pipeline,
            &vars,
            &provider,
            &config,
            &mut |_| {},
        )
        .expect("ejecucion");
        assert_eq!(
            con_pistas.final_tile,
            Some(128),
            "el tile tiene que salir de los candidatos del modelo"
        );
    }

    #[test]
    fn the_manifest_vram_per_megapixel_mandates_over_the_calibration() {
        // El modelo sabe lo que consume mejor que una calibracion hecha en otro
        // equipo: si declara 20000 MB por megapixel de tile, el presupuesto tiene
        // que bajarlo, aunque la calibracion dijera 100.
        let image = gradient(256, 256);
        let pipeline = single_upscale_pipeline("4x-anime");
        let vars = vars_for(Mode::Illustration);
        let config = RunnerConfig {
            budget: Some(VramBudget::for_provider(
                4096,
                70,
                100.0,
                su_tiling::ProviderKind::Cpu,
            )),
            ..RunnerConfig::default()
        };

        let provider = MockBackendProvider;
        let con_calibracion = run_pipeline(
            &image,
            &pipeline,
            &vars,
            &provider,
            &config,
            &mut |_| {},
        )
        .expect("ejecucion");
        assert_eq!(
            con_calibracion.final_tile,
            Some(1024),
            "con 100 MB por megapixel el tile mayor cabe de sobra"
        );

        let provider = HintsProvider {
            hints: hints(
                su_tiling::DEFAULT_CANDIDATES,
                16,
                32,
                Some(20_000.0),
            ),
        };
        let con_pistas = run_pipeline(
            &image,
            &pipeline,
            &vars,
            &provider,
            &config,
            &mut |_| {},
        )
        .expect("ejecucion");
        assert_eq!(
            con_pistas.final_tile,
            Some(256),
            "la VRAM del manifiesto tiene que mandar sobre la calibracion"
        );
    }

    #[test]
    fn with_hints_wires_every_field_and_ignores_nonsense() {
        let base = RunnerConfig::default();

        // Sin pistas, la configuracion se queda como esta.
        let igual = base.with_hints(None);
        assert_eq!(igual.candidates, base.candidates);
        assert_eq!(igual.overlap_divisor, base.overlap_divisor);
        assert_eq!(igual.pad_to, base.pad_to);
        assert_eq!(igual.vram_per_megapixel, None);

        let aplicada = base.with_hints(Some(&hints(&[512, 384], 8, 16, Some(300.0))));
        assert_eq!(aplicada.candidates, vec![512, 384]);
        assert_eq!(aplicada.overlap_divisor, 8);
        assert_eq!(aplicada.pad_to, 16);
        assert_eq!(aplicada.vram_per_megapixel, Some(300.0));

        // Y la configuracion de la que sale no se toca: cada etapa tiene su modelo.
        assert_eq!(base.candidates, su_tiling::DEFAULT_CANDIDATES);
        assert_eq!(base.overlap_divisor, 16);
        assert_eq!(base.pad_to, 32);
        assert_eq!(base.vram_per_megapixel, None);

        // Un manifiesto absurdo no puede dejar al planificador sin candidatos ni
        // colar valores que rompan la geometria.
        let absurda = base.with_hints(Some(&hints(&[8, 16], 0, 0, Some(0.0))));
        assert_eq!(absurda.candidates, base.candidates);
        assert_eq!(absurda.overlap_divisor, base.overlap_divisor);
        assert_eq!(absurda.pad_to, base.pad_to);
        assert_eq!(absurda.vram_per_megapixel, None);
    }

    // -----------------------------------------------------------------------
    // Enfoque
    // -----------------------------------------------------------------------

    #[test]
    fn unsharp_leaves_a_flat_image_untouched() {
        let mut image = gradient(64, 64);
        image.rgb.fill(0.5);

        let sharpened = unsharp(&image, 0.5, 1.0, 0.0).expect("enfoque");
        assert_eq!(sharpened.rgb, image.rgb);
    }

    #[test]
    fn unsharp_increases_the_contrast_of_an_edge() {
        let mut image = gradient(64, 64);
        // Mitad izquierda a 0.2, mitad derecha a 0.8.
        for y in 0..64 {
            for x in 0..64 {
                let index = ((y * 64 + x) * 3) as usize;
                let value = if x < 32 { 0.2 } else { 0.8 };
                image.rgb[index] = value;
                image.rgb[index + 1] = value;
                image.rgb[index + 2] = value;
            }
        }

        let sharpened = unsharp(&image, 0.8, 1.0, 0.0).expect("enfoque");

        // El lado claro debe aclararse y el oscuro oscurecerse. Se sondea **pegado al
        // escalon**: el desenfoque es una caja de radio 1 aplicada tres veces, asi
        // que solo alcanza tres pixeles a cada lado. Un punto a ocho pixeles del
        // borde no cambia, y eso es correcto —el enfoque no debe tocar las zonas
        // planas—, asi que sondearlo comprobaba lo contrario de lo que se quiere.
        let light_before = image.rgb[((32 * 64 + 32) * 3) as usize];
        let light_after = sharpened.rgb[((32 * 64 + 32) * 3) as usize];
        let dark_before = image.rgb[((32 * 64 + 31) * 3) as usize];
        let dark_after = sharpened.rgb[((32 * 64 + 31) * 3) as usize];

        assert!(light_after > light_before, "{light_after} <= {light_before}");
        assert!(dark_after < dark_before, "{dark_after} >= {dark_before}");
    }

    #[test]
    fn unsharp_respects_the_threshold() {
        // El umbral esta en la escala 0..1, la misma que `amount` y `blend`. Hasta
        // ahora se dividia por 255, asi que los 0,02 que declaran los pipelines
        // valian 0,0000784 y el filtro no filtraba nada: esta prueba pasaba con un
        // umbral de 50 (escala 0..255), que es la escala equivocada.
        let mut image = gradient(32, 32);
        image.rgb.fill(0.5);
        // Un detalle del 0,4 % de la escala: por debajo del umbral del 5 %.
        image.rgb[0] = 0.504;

        let con_umbral = unsharp(&image, 1.0, 1.0, 0.05).expect("enfoque");
        assert_eq!(
            con_umbral.rgb, image.rgb,
            "un detalle por debajo del umbral no se puede amplificar"
        );

        // Con el mismo detalle y sin umbral, si se amplifica: es la prueba de que
        // el umbral es lo unico que lo estaba frenando.
        let sin_umbral = unsharp(&image, 1.0, 1.0, 0.0).expect("enfoque");
        assert_ne!(sin_umbral.rgb, image.rgb);

        // Y un detalle por encima del umbral pasa el filtro con el umbral puesto.
        let mut escalon = gradient(32, 32);
        for y in 0..32 {
            for x in 0..32 {
                let index = ((y * 32 + x) * 3) as usize;
                let value = if x < 16 { 0.2 } else { 0.8 };
                escalon.rgb[index] = value;
                escalon.rgb[index + 1] = value;
                escalon.rgb[index + 2] = value;
            }
        }
        let con_umbral = unsharp(&escalon, 0.3, 1.0, 0.05).expect("enfoque");
        assert_ne!(
            con_umbral.rgb, escalon.rgb,
            "un escalon del 60 % tiene que pasar el umbral del 5 %"
        );
    }

    #[test]
    fn unsharp_with_zero_amount_is_a_no_op() {
        let image = gradient(32, 32);
        let sharpened = unsharp(&image, 0.0, 1.0, 0.0).expect("enfoque");
        assert_eq!(sharpened.rgb, image.rgb);
    }

    #[test]
    fn the_mock_provider_deduces_the_scale_from_the_model_id() {
        assert_eq!(MockBackendProvider::scale_for("4x-ultrasharp"), 4);
        assert_eq!(MockBackendProvider::scale_for("realesrgan-x4plus-anime-6b"), 4);
        assert_eq!(MockBackendProvider::scale_for("2x-animesharpv3"), 2);
        assert_eq!(MockBackendProvider::scale_for("8x-algo"), 8);
        // El token puede ir detras de la `x` en lugar de delante.
        assert_eq!(MockBackendProvider::scale_for("realesrgan-x4plus"), 4);
    }

    #[test]
    fn the_reference_provider_says_what_it_is_and_claims_no_vram() {
        // `name` y `uses_vram` son los dos datos que el informe y `EvalVars`
        // publican sobre el backend. Si el proveedor de referencia se anunciara
        // como un EP de GPU, el pipeline elegiria la rama de GPU y el resultado
        // diria "TensorRT" sin haberlo usado nunca.
        let provider = MockBackendProvider;
        assert_eq!(provider.name(), "referencia");
        assert!(!provider.uses_vram());
    }

    #[test]
    fn restoration_models_do_not_change_the_scale() {
        // Un modelo de restauracion devuelve una imagen del mismo tamano que la de
        // entrada. Si aqui saliera 4, el runner escalaria por 4 en la etapa de
        // denoise o de restauracion facial y el pipeline entero saldria a 16x
        // cuando el usuario pidio 4x, sin ningun error.
        assert_eq!(MockBackendProvider::scale_for("scunet-color"), 1);
        assert_eq!(MockBackendProvider::scale_for("gfpgan-v1.4"), 1);
        assert_eq!(MockBackendProvider::scale_for("codeformer"), 1);
        assert_eq!(MockBackendProvider::scale_for("yunet-face"), 1);
    }
}
