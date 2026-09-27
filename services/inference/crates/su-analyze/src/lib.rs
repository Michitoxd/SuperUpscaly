//! # su-analyze
//!
//! Analisis previo al escalado. Su salida alimenta las condiciones del pipeline:
//! si la imagen ya es limpia, no se le mete denoise; si tiene artefactos de
//! compresion, no se le aplica enfoque.
//!
//! ## Que se mide y que no
//!
//! En la Fase 2 se miden las tres cosas que se pueden calcular sin modelos:
//! **ruido**, **artefactos de compresion** y las propiedades del archivo
//! (resolucion, canal alfa, orientacion EXIF). La deteccion de rostros y el
//! clasificador foto/ilustracion necesitan redes neuronales y llegan en la
//! Fase 4; hasta entonces `kind` queda en `Unknown` con confianza 0, lo que hace
//! que el pipeline respete siempre la eleccion del usuario en lugar de
//! contradecirla con datos que no tiene.
//!
//! Los dos estimadores son puros y deterministicos: se pueden verificar con
//! imagenes sinteticas, sin depender de hardware ni de modelos.

use su_core::{Analysis, ContentKind, Scale};
use su_imageio::DecodedImage;

/// Ruido (sigma) que corresponde a la puntuacion 1.0.
///
/// Calibrado de forma que el umbral de los pipelines (`noise > 0.35`) se active
/// a partir de sigma ~0.017, que en la practica separa una foto limpia de una
/// degradada o muy comprimida.
pub const NOISE_REFERENCE: f32 = 0.05;

/// Puntuacion de artefactos a partir de la cual se inhibe el enfoque final.
pub const BLOCKINESS_REFERENCE: f32 = 3.0;

/// Analiza una imagen ya decodificada.
///
/// `scale` se usa solo para estimar los megapixeles de salida, que es lo que
/// permite avisar antes de generar un archivo de 3 gigapixeles por accidente.
pub fn analyze(image: &DecodedImage, scale: Scale) -> Analysis {
    let noise = estimate_noise(&image.rgb, image.width, image.height);
    let blockiness = estimate_blockiness(&image.rgb, image.width, image.height);

    Analysis {
        width: image.width,
        height: image.height,
        has_alpha: image.has_alpha(),
        noise,
        blockiness,
        // Sin clasificador no se puede afirmar nada del tipo de contenido.
        kind: ContentKind::Unknown,
        kind_confidence: 0.0,
        faces: Vec::new(),
        exif_orientation: image.applied_orientation,
        estimated_output_mp: image.megapixels() * (scale.factor() * scale.factor()) as f32,
    }
}

/// Luma aproximada de un pixel, en `0..1`.
#[inline]
fn luma(rgb: &[f32], pixel: usize) -> f32 {
    let base = pixel * 3;
    // Coeficientes de Rec. 709.
    0.2126 * rgb[base] + 0.7152 * rgb[base + 1] + 0.0722 * rgb[base + 2]
}

/// Estima el ruido a partir de la respuesta del laplaciano.
///
/// El laplaciano es un filtro paso alto: en una zona plana su respuesta es
/// proporcional al ruido, no al contenido. Dividiendo su desviacion tipica por la
/// norma del kernel (`sqrt(20)` para el laplaciano de 3x3) se obtiene una
/// estimacion directa de sigma, sin necesidad de umbrales ni de iteraciones.
pub fn estimate_noise(rgb: &[f32], width: u32, height: u32) -> f32 {
    if width < 3 || height < 3 {
        return 0.0;
    }

    // Se muestrea con paso 2 en cada eje: el ruido es estacionario y la
    // estimacion apenas cambia, pero el coste baja a la cuarta parte.
    let step = 2usize;
    let mut sum = 0.0f64;
    let mut sum_squares = 0.0f64;
    let mut samples = 0u64;

    let mut y = 1usize;
    while y + 1 < height as usize {
        let mut x = 1usize;
        while x + 1 < width as usize {
            let row = y * width as usize;
            let center = luma(rgb, row + x);
            let up = luma(rgb, (y - 1) * width as usize + x);
            let down = luma(rgb, (y + 1) * width as usize + x);
            let left = luma(rgb, row + x - 1);
            let right = luma(rgb, row + x + 1);

            let response = (up + down + left + right - 4.0 * center) as f64;
            sum += response;
            sum_squares += response * response;
            samples += 1;

            x += step;
        }
        y += step;
    }

    if samples < 4 {
        return 0.0;
    }

    let count = samples as f64;
    let mean = sum / count;
    let variance = (sum_squares / count) - (mean * mean);
    let std_dev = variance.max(0.0).sqrt();

    // sqrt(1 + 1 + 1 + 1 + 16) = sqrt(20)
    let sigma = std_dev / 20.0f64.sqrt();

    ((sigma as f32) / NOISE_REFERENCE).clamp(0.0, 1.0)
}

/// Estima los artefactos de compresion por bloques.
///
/// JPEG comprime en bloques de 8x8, lo que deja discontinuidades en las
/// fronteras de bloque que no aparecen en el resto de la imagen. La razon entre
/// la diferencia media en las fronteras y la del resto es un detector directo:
/// vale ~1 en una imagen limpia y crece con la compresion.
pub fn estimate_blockiness(rgb: &[f32], width: u32, height: u32) -> f32 {
    if width < 16 || height < 16 {
        return 0.0;
    }

    let width_usize = width as usize;
    let mut boundary_sum = 0.0f64;
    let mut boundary_count = 0u64;
    let mut interior_sum = 0.0f64;
    let mut interior_count = 0u64;

    // Diferencias horizontales.
    for y in 0..height as usize {
        let row = y * width_usize;
        for x in 1..width_usize {
            let difference = (luma(rgb, row + x) - luma(rgb, row + x - 1)).abs() as f64;
            // La frontera esta entre x-1 y x, es decir en la columna x.
            if x % 8 == 0 {
                boundary_sum += difference;
                boundary_count += 1;
            } else {
                interior_sum += difference;
                interior_count += 1;
            }
        }
    }

    // Diferencias verticales.
    for y in 1..height as usize {
        for x in 0..width_usize {
            let difference =
                (luma(rgb, y * width_usize + x) - luma(rgb, (y - 1) * width_usize + x)).abs()
                    as f64;
            if y % 8 == 0 {
                boundary_sum += difference;
                boundary_count += 1;
            } else {
                interior_sum += difference;
                interior_count += 1;
            }
        }
    }

    if boundary_count == 0 || interior_count == 0 {
        return 0.0;
    }

    let boundary_mean = boundary_sum / boundary_count as f64;
    let interior_mean = interior_sum / interior_count as f64;

    // Una imagen plana (interior ~0) no debe dar una puntuacion enorme por una
    // division casi por cero.
    if interior_mean < 1e-6 {
        return if boundary_mean < 1e-6 { 0.0 } else { 1.0 };
    }

    let ratio = boundary_mean / interior_mean;
    (((ratio - 1.0) / (BLOCKINESS_REFERENCE as f64 - 1.0)) as f32).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generador congruencial lineal: ruido reproducible sin anadir `rand`.
    struct Lcg(u64);

    impl Lcg {
        fn next_f32(&mut self) -> f32 {
            self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            ((self.0 >> 33) as f32) / (u32::MAX as f32 / 2.0) - 1.0
        }
    }

    // `FnMut` y no `Fn`: los tests que necesitan ruido reproducible capturan un
    // generador en la closure y avanzan su estado en cada pixel.
    fn image_from(width: u32, height: u32, mut f: impl FnMut(u32, u32) -> f32) -> DecodedImage {
        let mut rgb = Vec::with_capacity((width as usize) * (height as usize) * 3);
        for y in 0..height {
            for x in 0..width {
                let value = f(x, y);
                rgb.push(value);
                rgb.push(value);
                rgb.push(value);
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

    #[test]
    fn a_smooth_gradient_is_not_noisy() {
        let image = image_from(128, 128, |x, y| (x + y) as f32 / 256.0);
        let noise = estimate_noise(&image.rgb, image.width, image.height);
        assert!(noise < 0.2, "un degradado suave dio ruido {noise}");
    }

    #[test]
    fn a_flat_image_has_no_noise() {
        let image = image_from(64, 64, |_, _| 0.5);
        assert_eq!(estimate_noise(&image.rgb, image.width, image.height), 0.0);
    }

    #[test]
    fn a_noisy_image_scores_higher_than_a_clean_one() {
        let clean = image_from(128, 128, |x, y| ((x + y) as f32 / 256.0).clamp(0.2, 0.8));

        let mut rng = Lcg(12345);
        let noisy = image_from(128, 128, |x, y| {
            let base = ((x + y) as f32 / 256.0).clamp(0.2, 0.8);
            (base + rng.next_f32() * 0.15).clamp(0.0, 1.0)
        });

        let clean_noise = estimate_noise(&clean.rgb, clean.width, clean.height);
        let noisy_noise = estimate_noise(&noisy.rgb, noisy.width, noisy.height);

        assert!(
            noisy_noise > clean_noise * 3.0,
            "limpia {clean_noise}, ruidosa {noisy_noise}"
        );
        assert!(noisy_noise > 0.3, "una imagen con ruido visible deberia superar el umbral: {noisy_noise}");
    }

    #[test]
    fn the_noise_score_stays_within_range() {
        let mut rng = Lcg(99);
        let extreme = image_from(64, 64, |_, _| rng.next_f32());
        let noise = estimate_noise(&extreme.rgb, extreme.width, extreme.height);
        assert!((0.0..=1.0).contains(&noise), "puntuacion fuera de rango: {noise}");
    }

    #[test]
    fn tiny_images_do_not_panic() {
        let image = image_from(2, 2, |_, _| 0.5);
        assert_eq!(estimate_noise(&image.rgb, 2, 2), 0.0);
        assert_eq!(estimate_blockiness(&image.rgb, 2, 2), 0.0);
    }

    #[test]
    fn a_smooth_image_has_no_block_artefacts() {
        let image = image_from(128, 128, |x, y| ((x + y) as f32 / 256.0).clamp(0.1, 0.9));
        let blockiness = estimate_blockiness(&image.rgb, image.width, image.height);
        assert!(blockiness < 0.3, "un degradado suave dio blockiness {blockiness}");
    }

    #[test]
    fn block_boundaries_are_detected() {
        // Cada bloque de 8x8 tiene un valor distinto: las fronteras destacan
        // muchisimo sobre el interior, que es plano.
        let image = image_from(128, 128, |x, y| {
            let block = (x / 8) * 16 + (y / 8);
            (block as f32 / 256.0).clamp(0.05, 0.95)
        });

        let blockiness = estimate_blockiness(&image.rgb, image.width, image.height);
        assert!(
            blockiness > 0.5,
            "una imagen con bloques marcados deberia dar blockiness alta: {blockiness}"
        );
    }

    #[test]
    fn a_flat_image_does_not_report_false_blockiness() {
        let image = image_from(64, 64, |_, _| 0.5);
        assert_eq!(estimate_blockiness(&image.rgb, image.width, image.height), 0.0);
    }

    #[test]
    fn analysis_reports_the_geometry_and_the_output_size() {
        let image = image_from(1000, 500, |x, y| ((x + y) as f32 / 1500.0).clamp(0.0, 1.0));
        let analysis = analyze(&image, Scale::X4);

        assert_eq!(analysis.width, 1000);
        assert_eq!(analysis.height, 500);
        assert!(!analysis.has_alpha);
        // 1000x500 = 0.5 MP, x4 de lado -> 16x de area -> 8 MP.
        assert!((analysis.estimated_output_mp - 8.0).abs() < 0.01);
    }

    #[test]
    fn without_a_classifier_the_content_kind_is_unknown() {
        // Es deliberado: con confianza 0, `contradicts` nunca puede contradecir al
        // usuario basandose en datos que no existen.
        let image = image_from(64, 64, |_, _| 0.5);
        let analysis = analyze(&image, Scale::X2);

        assert_eq!(analysis.kind, ContentKind::Unknown);
        assert_eq!(analysis.kind_confidence, 0.0);
        assert!(!analysis.contradicts(ContentKind::Photo));
        assert!(!analysis.contradicts(ContentKind::Illustration));
    }

    #[test]
    fn the_eight_x_output_size_is_computed_correctly() {
        let image = image_from(512, 512, |_, _| 0.5);
        let analysis = analyze(&image, Scale::X8);
        // 512x512 = 0.262 MP, x8 de lado -> 64x de area -> 16.8 MP.
        assert!((analysis.estimated_output_mp - 16.777).abs() < 0.01);
    }

    #[test]
    fn the_noise_estimate_feeds_the_denoise_threshold_sensibly() {
        // Comprobacion de coherencia entre la escala del estimador y el umbral que
        // usan los pipelines (0.35). Si alguien recalibra uno sin el otro, este
        // test lo detecta.
        assert!(
            (0.0..1.0).contains(&NOISE_REFERENCE),
            "la referencia de ruido debe ser una sigma realista"
        );
        let threshold_sigma = 0.35 * NOISE_REFERENCE;
        assert!(
            (0.01..0.03).contains(&threshold_sigma),
            "el umbral de denoise corresponde a sigma {threshold_sigma}, fuera de lo razonable"
        );
    }
}
