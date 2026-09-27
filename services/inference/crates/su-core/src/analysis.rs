//! Resultado del analisis previo al escalado.
//!
//! Se ejecuta sobre una copia reducida de la imagen (lado mayor <= 1024 px) para
//! que cueste menos de 150 ms tipicamente. Su salida decide que etapas del
//! pipeline se activan: si la imagen ya es limpia, no se le mete denoise.

use serde::{Deserialize, Serialize};

use crate::types::ContentKind;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceBox {
    /// Coordenadas normalizadas 0..1 respecto a la imagen analizada.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub confidence: f32,
}

impl FaceBox {
    /// Una caja es utilizable si tiene area real y confianza suficiente.
    pub fn is_reliable(&self) -> bool {
        self.confidence >= 0.5 && self.w > 0.01 && self.h > 0.01
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Analysis {
    pub width: u32,
    pub height: u32,
    pub has_alpha: bool,

    /// Sigma de ruido estimado, normalizado 0..1 (estimador wavelet de Donoho).
    pub noise: f32,
    /// Artefactos de compresion JPEG, normalizado 0..1.
    pub blockiness: f32,

    pub kind: ContentKind,
    /// Confianza de `kind`, 0..1. Por debajo de 0.80 no se contradice al usuario.
    pub kind_confidence: f32,

    pub faces: Vec<FaceBox>,

    /// Valor EXIF de orientacion (1 = normal). Se aplica antes de inferir.
    pub exif_orientation: u16,

    /// Megapixeles que tendra la salida con el factor de escala solicitado.
    pub estimated_output_mp: f32,
}

impl Default for Analysis {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            has_alpha: false,
            noise: 0.0,
            blockiness: 0.0,
            kind: ContentKind::Unknown,
            kind_confidence: 0.0,
            faces: Vec::new(),
            exif_orientation: 1,
            estimated_output_mp: 0.0,
        }
    }
}

impl Analysis {
    /// Rostros que se tendran en cuenta para la restauracion facial.
    pub fn reliable_face_count(&self) -> usize {
        self.faces.iter().filter(|face| face.is_reliable()).count()
    }

    /// La orientacion EXIF obliga a rotar si no es 1 (o 0 si el archivo no la trae).
    pub fn needs_rotation(&self) -> bool {
        !matches!(self.exif_orientation, 0 | 1)
    }

    /// Umbral por encima del cual merece la pena reducir ruido antes de escalar.
    pub const DENOISE_THRESHOLD: f32 = 0.35;
    /// Umbral por encima del cual el enfoque final amplificaria artefactos.
    pub const BLOCKINESS_THRESHOLD: f32 = 0.25;
    /// Por debajo de esta confianza, el tipo detectado no contradice al usuario.
    pub const KIND_CONFIDENCE_THRESHOLD: f32 = 0.80;

    pub fn suggests_denoise(&self) -> bool {
        self.noise > Self::DENOISE_THRESHOLD
    }

    pub fn suggests_no_sharpen(&self) -> bool {
        self.blockiness > Self::BLOCKINESS_THRESHOLD
    }

    /// El analisis contradice la eleccion del usuario de forma clara.
    pub fn contradicts(&self, chosen: ContentKind) -> bool {
        self.kind != ContentKind::Unknown
            && chosen != ContentKind::Unknown
            && self.kind != chosen
            && self.kind_confidence > Self::KIND_CONFIDENCE_THRESHOLD
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analysis_with(noise: f32, blockiness: f32, faces: usize) -> Analysis {
        Analysis {
            noise,
            blockiness,
            faces: (0..faces)
                .map(|_| FaceBox {
                    x: 0.1,
                    y: 0.1,
                    w: 0.2,
                    h: 0.2,
                    confidence: 0.9,
                })
                .collect(),
            ..Analysis::default()
        }
    }

    #[test]
    fn noisy_images_suggest_denoise() {
        assert!(!analysis_with(0.10, 0.0, 0).suggests_denoise());
        assert!(analysis_with(0.60, 0.0, 0).suggests_denoise());
        // Justo en el umbral no se activa: el umbral es estricto.
        assert!(!analysis_with(Analysis::DENOISE_THRESHOLD, 0.0, 0).suggests_denoise());
    }

    #[test]
    fn blocky_images_suppress_sharpening() {
        assert!(!analysis_with(0.0, 0.10, 0).suggests_no_sharpen());
        assert!(analysis_with(0.0, 0.40, 0).suggests_no_sharpen());
    }

    #[test]
    fn unreliable_face_boxes_are_ignored() {
        let mut analysis = analysis_with(0.0, 0.0, 1);
        assert_eq!(analysis.reliable_face_count(), 1);
        analysis.faces[0].confidence = 0.2;
        assert_eq!(analysis.reliable_face_count(), 0);
    }

    #[test]
    fn exif_orientation_1_needs_no_rotation() {
        let mut analysis = Analysis::default();
        assert!(!analysis.needs_rotation());
        analysis.exif_orientation = 6;
        assert!(analysis.needs_rotation());
    }

    #[test]
    fn kind_only_contradicts_with_high_confidence() {
        let mut analysis = analysis_with(0.0, 0.0, 0);
        analysis.kind = ContentKind::Illustration;
        analysis.kind_confidence = 0.60;
        assert!(!analysis.contradicts(ContentKind::Photo));

        analysis.kind_confidence = 0.95;
        assert!(analysis.contradicts(ContentKind::Photo));
        // No se contradice a si mismo ni cuando el analisis no sabe nada.
        assert!(!analysis.contradicts(ContentKind::Illustration));
        analysis.kind = ContentKind::Unknown;
        assert!(!analysis.contradicts(ContentKind::Photo));
    }
}
