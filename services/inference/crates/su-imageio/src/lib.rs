//! # su-imageio
//!
//! Entrada y salida de imagen, con tres decisiones que no son negociables:
//!
//! 1. **La orientacion EXIF se aplica antes de inferir** (ADR-013). Si no, la
//!    deteccion de rostros falla en fotos de movil y la salida sale girada.
//! 2. **La escritura es atomica** (ADR-015): se escribe en un temporal, se
//!    sincroniza y se renombra. Un fallo a mitad no deja un archivo a medias que
//!    el usuario crea valido.
//! 3. **La salida se valida antes de escribirse.** Un buffer uniforme, con `NaN`
//!    o con el rango dinamico colapsado no llega al disco: se reporta como error.
//!
//! El canal alfa se trata aparte: se escala por separado y se recompone, en lugar
//! de meterlo en el modelo como un cuarto canal que ningun modelo espera.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageFormat, RgbImage, RgbaImage};
use su_core::{OutputFormat, SuError, SuResult};

/// Sufijo del archivo temporal de escritura.
const TEMP_SUFFIX: &str = ".su-tmp";

/// Distingue dos escrituras simultaneas en el mismo proceso.
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Desviacion estandar de luminancia por debajo de la cual la imagen se considera
/// uniforme. Una imagen real siempre supera este valor con holgura.
const MIN_LUMA_STD_DEV: f32 = 1.0;

/// Coeficientes de luma de Rec. 709.
///
/// La validacion de "imagen uniforme" tiene que medir **luma**, no la media de los
/// tres canales mezclados: un rojo plano uniforme (RGB `(1, 0, 0)`) es una imagen
/// plana, pero mezclando canales su desviacion sale 121/255 y pasaba el filtro.
const LUMA_RED: f32 = 0.2126;
const LUMA_GREEN: f32 = 0.7152;
const LUMA_BLUE: f32 = 0.0722;

/// Imagen ya decodificada y normalizada, lista para el pipeline.
///
/// Los canales se guardan como `f32` en `0..1` porque es lo que espera cualquier
/// modelo de upscaling y porque evita cuantizar dos veces en un pipeline de varias
/// etapas (denoise -> upscale -> face -> sharpen).
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    /// RGB entrelazado, `width * height * 3` valores en `0..1`.
    pub rgb: Vec<f32>,
    /// Canal alfa, `width * height` valores en `0..1`, si la imagen lo tenia.
    pub alpha: Option<Vec<f32>>,
    /// Perfil ICC tal cual venia, para poder conservarlo en la salida.
    pub icc_profile: Option<Vec<u8>>,
    /// Valor EXIF de orientacion **ya aplicado**; se conserva solo para informar.
    pub applied_orientation: u16,
}

impl DecodedImage {
    pub fn pixel_count(&self) -> usize {
        (self.width as usize) * (self.height as usize)
    }

    pub fn has_alpha(&self) -> bool {
        self.alpha.is_some()
    }

    pub fn megapixels(&self) -> f32 {
        (self.pixel_count() as f32) / 1_000_000.0
    }

    /// Recompone un `DynamicImage` en `f32`, sin cuantizar.
    ///
    /// Es el que usa el reescalado. Se separa de [`Self::to_dynamic_image`] porque
    /// aquel es para **escribir** (los codificadores de PNG/JPEG/WebP trabajan en
    /// 8 bits por canal) y este para **calcular**.
    pub fn to_dynamic_image_f32(&self) -> SuResult<DynamicImage> {
        let expected = self.pixel_count() * 3;
        if self.rgb.len() != expected {
            return Err(SuError::Internal(format!(
                "buffer RGB de {} valores, se esperaban {expected}",
                self.rgb.len()
            )));
        }

        match &self.alpha {
            None => {
                let buffer = image::Rgb32FImage::from_raw(self.width, self.height, self.rgb.clone())
                    .ok_or_else(|| {
                        SuError::Internal("no se pudo reconstruir la imagen RGB en f32".to_string())
                    })?;
                Ok(DynamicImage::ImageRgb32F(buffer))
            }
            Some(alpha) => {
                if alpha.len() != self.pixel_count() {
                    return Err(SuError::Internal(format!(
                        "canal alfa de {} valores, se esperaban {}",
                        alpha.len(),
                        self.pixel_count()
                    )));
                }

                let mut rgba = vec![0.0f32; self.pixel_count() * 4];
                for index in 0..self.pixel_count() {
                    rgba[index * 4] = self.rgb[index * 3];
                    rgba[index * 4 + 1] = self.rgb[index * 3 + 1];
                    rgba[index * 4 + 2] = self.rgb[index * 3 + 2];
                    rgba[index * 4 + 3] = alpha[index];
                }

                let buffer = image::Rgba32FImage::from_raw(self.width, self.height, rgba)
                    .ok_or_else(|| {
                        SuError::Internal("no se pudo reconstruir la imagen RGBA en f32".to_string())
                    })?;
                Ok(DynamicImage::ImageRgba32F(buffer))
            }
        }
    }

    /// Recompone un `DynamicImage` de 8 bits para poder **guardarlo** con `image`.
    pub fn to_dynamic_image(&self) -> SuResult<DynamicImage> {
        let expected = self.pixel_count() * 3;
        if self.rgb.len() != expected {
            return Err(SuError::Internal(format!(
                "buffer RGB de {} valores, se esperaban {expected}",
                self.rgb.len()
            )));
        }

        let mut rgb8 = vec![0u8; expected];
        for (target, value) in rgb8.iter_mut().zip(self.rgb.iter()) {
            *target = to_u8(*value);
        }

        match &self.alpha {
            None => {
                let buffer = RgbImage::from_raw(self.width, self.height, rgb8).ok_or_else(|| {
                    SuError::Internal("no se pudo reconstruir la imagen RGB".to_string())
                })?;
                Ok(DynamicImage::ImageRgb8(buffer))
            }
            Some(alpha) => {
                if alpha.len() != self.pixel_count() {
                    return Err(SuError::Internal(format!(
                        "canal alfa de {} valores, se esperaban {}",
                        alpha.len(),
                        self.pixel_count()
                    )));
                }
                let mut rgba8 = vec![0u8; self.pixel_count() * 4];
                for index in 0..self.pixel_count() {
                    rgba8[index * 4] = rgb8[index * 3];
                    rgba8[index * 4 + 1] = rgb8[index * 3 + 1];
                    rgba8[index * 4 + 2] = rgb8[index * 3 + 2];
                    rgba8[index * 4 + 3] = to_u8(alpha[index]);
                }
                let buffer = RgbaImage::from_raw(self.width, self.height, rgba8).ok_or_else(|| {
                    SuError::Internal("no se pudo reconstruir la imagen RGBA".to_string())
                })?;
                Ok(DynamicImage::ImageRgba8(buffer))
            }
        }
    }
}

/// Convierte `0..1` a `0..255` saturando. Un `NaN` se convierte en 0 en lugar de
/// en 128 (que es lo que hace `as u8` de forma sorprendente en algunos casos).
fn to_u8(value: f32) -> u8 {
    if value.is_nan() {
        return 0;
    }
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Convierte un `DynamicImage` al formato interno.
pub fn from_dynamic_image(image: &DynamicImage) -> DecodedImage {
    let (width, height) = image.dimensions();
    let has_alpha = image.color().has_alpha();

    if has_alpha {
        let rgba = image.to_rgba32f();
        let count = (width as usize) * (height as usize);
        let mut rgb = Vec::with_capacity(count * 3);
        let mut alpha = Vec::with_capacity(count);
        for pixel in rgba.pixels() {
            rgb.push(pixel[0]);
            rgb.push(pixel[1]);
            rgb.push(pixel[2]);
            alpha.push(pixel[3]);
        }
        DecodedImage {
            width,
            height,
            rgb,
            alpha: Some(alpha),
            icc_profile: None,
            applied_orientation: 1,
        }
    } else {
        let rgb_buffer = image.to_rgb32f();
        let mut rgb = Vec::with_capacity(rgb_buffer.len());
        for pixel in rgb_buffer.pixels() {
            rgb.push(pixel[0]);
            rgb.push(pixel[1]);
            rgb.push(pixel[2]);
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
}

/// Lee la orientacion EXIF sin decodificar los pixeles.
///
/// Cualquier problema (sin EXIF, archivo truncado, campo ausente) se resuelve con
/// `1`, que significa "sin rotacion". La orientacion nunca puede impedir abrir
/// una imagen.
pub fn read_exif_orientation(path: &Path) -> u16 {
    let Ok(file) = File::open(path) else {
        return 1;
    };
    let mut reader = BufReader::new(file);
    let Ok(exif) = exif::Reader::new().read_from_container(&mut reader) else {
        return 1;
    };
    exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
        .and_then(|field| field.value.get_uint(0))
        .map(|value| value as u16)
        .filter(|value| (1..=8).contains(value))
        .unwrap_or(1)
}

/// Aplica una orientacion EXIF a una imagen.
///
/// Los ocho valores del estandar se corresponden con rotaciones y reflexiones:
/// 1 normal, 2 espejo horizontal, 3 giro 180, 4 espejo vertical, 5 transpuesta,
/// 6 giro 90, 7 transversal, 8 giro 270.
pub fn apply_orientation(image: DynamicImage, orientation: u16) -> DynamicImage {
    match orientation {
        2 => image.fliph(),
        3 => image.rotate180(),
        4 => image.flipv(),
        5 => image.rotate90().fliph(),
        6 => image.rotate90(),
        7 => image.rotate270().fliph(),
        8 => image.rotate270(),
        _ => image,
    }
}

/// Decodifica un archivo aplicando ya la orientacion EXIF.
pub fn decode(path: &Path) -> SuResult<DecodedImage> {
    if !path.exists() {
        return Err(SuError::PathUnavailable(path.display().to_string()));
    }

    let orientation = read_exif_orientation(path);

    let reader = image::ImageReader::open(path)
        .map_err(|error| SuError::DecodeFailed(format!("{}: {error}", path.display())))?
        .with_guessed_format()
        .map_err(|error| SuError::DecodeFailed(format!("{}: {error}", path.display())))?;

    let decoded = reader.decode().map_err(|error| {
        // `image` distingue entre formato no soportado y archivo corrupto, pero
        // no expone esa distincion de forma estable: se inspecciona el mensaje.
        let text = error.to_string();
        if text.contains("format") || text.contains("unsupported") {
            SuError::UnsupportedFormat(format!("{}: {text}", path.display()))
        } else {
            SuError::CorruptFile(format!("{}: {text}", path.display()))
        }
    })?;

    let mut result = from_dynamic_image(&apply_orientation(decoded, orientation));
    result.applied_orientation = orientation;
    Ok(result)
}

/// Da color a los pixeles totalmente transparentes copiandolo del vecino visible
/// mas cercano.
///
/// ## Por que
///
/// Un pixel con alfa cero **no tiene color**: no se ve, y su valor RGB no
/// significa nada (depende de como se genero el archivo: negro, blanco, el color
/// del fondo que se recorto). El modelo, en cambio, no lo sabe: recibe el RGB tal
/// cual y reconstruye un borde entre ese color inventado y el dibujo. El resultado
/// es un halo del color del fondo transparente pegado al contorno — el defecto
/// clasico de los PNG recortados.
///
/// Extender el color del contorno hacia dentro de la zona transparente quita ese
/// borde falso: el modelo ve el dibujo continuo y deduce el contorno del alfa, que
/// es donde de verdad esta. Es lo mismo que hace un relleno por difusion antes de
/// recortar.
///
/// ## Lo que **no** hace
///
/// No toca los pixeles con alfa distinto de cero, ni el propio canal alfa: un
/// pixel semitransparente si aporta color a la mezcla, y cambiarlo cambiaria la
/// imagen compuesta que ve el usuario. Aqui solo se rellena lo que no se ve.
///
/// Sin nada que rellenar devuelve la imagen **prestada**, no una copia: devolver
/// una copia del RGB costaba 240 MB por imagen de 20 MP en el caso mas comun (una
/// foto sin canal alfa, o un PNG con el alfa entero a 1), y el llamador solo
/// necesita leerla.
pub fn bleed_transparent(image: &DecodedImage) -> Cow<'_, DecodedImage> {
    let Some(alpha) = image.alpha.as_ref() else {
        return Cow::Borrowed(image);
    };

    if alpha.len() != image.pixel_count() {
        // Un buffer incoherente no se arregla aqui: se devuelve la entrada y que
        // falle donde de verdad se usa el alfa.
        return Cow::Borrowed(image);
    }

    // "Visible" es cualquier pixel con alfa mayor que cero, por infinitesimal que
    // sea. La alternativa —un umbral, por ejemplo 0.5— introduciria un parametro
    // que el usuario no puede elegir y que casi nunca cambia nada: en la practica
    // el alfa real es 0 o 1, y en un degradado de pluma el color viene del interior
    // visible. Con el criterio estricto, un pixel casi transparente se trata como
    // color de verdad —lo que aporta su propio tono a la difusion— en lugar de
    // como un agujero.
    let mut visited: Vec<bool> = alpha.iter().map(|value| *value > 0.0).collect();

    // Nada que rellenar: o todos los pixeles son visibles —un PNG con el alfa
    // entero a 1, que es la mitad de los que llegan— o no lo es ninguno. En los dos
    // casos la difusion no cambiaria ni un pixel.
    let visibles = visited.iter().filter(|visible| **visible).count();
    if visibles == 0 || visibles == visited.len() {
        return Cow::Borrowed(image);
    }

    let width = image.width as usize;
    let height = image.height as usize;

    let mut filled = image.rgb.clone();

    // Frontera inicial: los pixeles que si tienen color. No puede estar vacia:
    // arriba se ha descartado el caso de que no haya ninguno visible.
    let mut frontier: VecDeque<u32> = (0..alpha.len() as u32)
        .filter(|index| visited[*index as usize])
        .collect();

    // Difusion por niveles: cada vuelta rellena exactamente un pixel de distancia,
    // asi que el color que gana es siempre el del pixel visible mas cercano.
    while !frontier.is_empty() {
        let mut next: VecDeque<u32> = VecDeque::new();

        while let Some(index) = frontier.pop_front() {
            let index = index as usize;
            let x = index % width;
            let y = index / width;

            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }

                    let nx = x as i64 + dx;
                    let ny = y as i64 + dy;

                    if nx < 0 || ny < 0 || nx >= width as i64 || ny >= height as i64 {
                        continue;
                    }

                    let neighbour = ny as usize * width + nx as usize;
                    if visited[neighbour] {
                        continue;
                    }

                    visited[neighbour] = true;
                    let source = index * 3;
                    let target = neighbour * 3;
                    // El color se copia a un temporal: los dos tramos son del mismo
                    // buffer y el prestatario no puede demostrar que no se solapan.
                    let colour = [filled[source], filled[source + 1], filled[source + 2]];
                    filled[target..target + 3].copy_from_slice(&colour);
                    next.push_back(neighbour as u32);
                }
            }
        }

        frontier = next;
    }

    Cow::Owned(DecodedImage {
        width: image.width,
        height: image.height,
        rgb: filled,
        alpha: image.alpha.clone(),
        icc_profile: image.icc_profile.clone(),
        applied_orientation: image.applied_orientation,
    })
}

/// Filtro de interpolacion del reescalado.
///
/// Se expone porque es una decision de calidad visible, no un detalle interno: el
/// pipeline lo declara por etapa con `"kernel"` y un usuario puede cambiarlo sin
/// recompilar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResizeKernel {
    /// El mas nitido de los suaves. Es el valor por defecto del proyecto.
    #[default]
    Lanczos3,
    /// Bicubica de Catmull-Rom: el "bicubic sharper" clasico. Conserva mas
    /// detalle que la bicubica simetrica y menos anillo que Lanczos3.
    CatmullRom,
    /// Gaussiana. La mas suave: util en fotos con ruido, donde marcar los bordes
    /// solo amplifica el grano.
    Gaussian,
    /// Lineal. Queda como referencia, no como opcion recomendada.
    Triangle,
    /// Sin interpolacion. Util para arte de pixeles y para tests de geometria.
    Nearest,
}

impl ResizeKernel {
    /// Nombres aceptados en el campo `kernel` de una etapa del pipeline.
    ///
    /// `bicubic` se acepta como sinonimo de `catmullrom` porque es el nombre que
    /// usa medio mundo para esta familia de filtros; el nombre canonico es el de
    /// la familia concreta, que es lo que se guarda en el informe.
    pub fn parse(text: &str) -> SuResult<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "lanczos3" | "lanczos" => Ok(Self::Lanczos3),
            "catmullrom" | "catmull-rom" | "catmull" | "bicubic" => Ok(Self::CatmullRom),
            "gaussian" | "gauss" => Ok(Self::Gaussian),
            "triangle" | "linear" | "bilinear" => Ok(Self::Triangle),
            "nearest" | "vecino" => Ok(Self::Nearest),
            other => Err(SuError::Internal(format!(
                "kernel de reescalado desconocido: '{other}' (validos: lanczos3, catmullrom, gaussian, triangle, nearest)"
            ))),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lanczos3 => "lanczos3",
            Self::CatmullRom => "catmullrom",
            Self::Gaussian => "gaussian",
            Self::Triangle => "triangle",
            Self::Nearest => "nearest",
        }
    }

    const fn filter_type(self) -> FilterType {
        match self {
            Self::Lanczos3 => FilterType::Lanczos3,
            Self::CatmullRom => FilterType::CatmullRom,
            Self::Gaussian => FilterType::Gaussian,
            Self::Triangle => FilterType::Triangle,
            Self::Nearest => FilterType::Nearest,
        }
    }
}

/// Reescala con el filtro indicado, **en coma flotante**.
///
/// ## Por que en `f32` y no en `u8`
///
/// La version anterior pasaba por `to_dynamic_image()`, que cuantiza a 8 bits por
/// canal, y reescalaba eso. Dos consecuencias, las dos visibles:
///
/// - **Precision.** Cada etapa de reescalado redondeaba a 1/255, y un pipeline de
///   varias etapas acumulaba el error. La descripcion de `photo:2x` promete
///   "reducir con Lanczos en 16 bits"; el codigo hacia 8, que no es lo mismo.
/// - **Nitidez.** La reduccion de 4x de una etapa de restauracion es un filtro
///   paso bajo; hacerlo sobre datos ya cuantizados pierde el margen que permite
///   recuperar la imagen con el `blend`.
///
/// `image` acepta `Rgb32FImage`/`Rgba32FImage`, asi que no hace falta implementar
/// el filtro a mano: se le da la imagen sin cuantizar y se recupera igual.
pub fn resize(
    image: &DecodedImage,
    width: u32,
    height: u32,
    kernel: ResizeKernel,
) -> SuResult<DecodedImage> {
    if width == 0 || height == 0 {
        return Err(SuError::Internal(
            "no se puede reescalar a un tamano vacio".to_string(),
        ));
    }

    // Reescalar al mismo tamano no cambia nada: se devuelve la entrada tal cual
    // en lugar de pasar por un filtro que solo puede perder precision.
    if width == image.width && height == image.height {
        return Ok(image.clone());
    }

    let source = image.to_dynamic_image_f32()?;
    let resized = source.resize_exact(width, height, kernel.filter_type());

    // El perfil ICC y la orientacion no son pixeles: `from_dynamic_image` no los
    // puede inventar y perdian en cada reescalado. El resultado se guardaba sin
    // perfil de color, que es un cambio de aspecto visible en imagenes con ICC.
    let mut result = from_dynamic_image(&resized);
    result.icc_profile = image.icc_profile.clone();
    result.applied_orientation = image.applied_orientation;
    Ok(result)
}

/// Recorta una region rectangular.
///
/// Se usa en la restauracion facial, que trabaja sobre recortes en lugar de sobre
/// la imagen entera. El recorte tiene que caber: un rectangulo fuera de la imagen
/// es un error de calculo del llamador, no algo que se pueda recortar "casi".
///
/// Se conservan el perfil ICC y la orientacion, para que el recorte siga siendo la
/// misma imagen y no una copia sin sus metadatos si alguien decide escribirlo.
pub fn crop(image: &DecodedImage, left: u32, top: u32, width: u32, height: u32) -> SuResult<DecodedImage> {
    if width == 0 || height == 0 {
        return Err(SuError::Internal(
            "no se puede recortar a un tamano vacio".to_string(),
        ));
    }

    if left + width > image.width || top + height > image.height {
        return Err(SuError::Internal(format!(
            "el recorte {width}x{height} en ({left}, {top}) se sale de la imagen {}x{}",
            image.width, image.height
        )));
    }

    let mut rgb = Vec::with_capacity((width as usize) * (height as usize) * 3);
    for y in 0..height {
        let row = ((top + y) as usize) * (image.width as usize) * 3;
        let start = row + (left as usize) * 3;
        rgb.extend_from_slice(&image.rgb[start..start + (width as usize) * 3]);
    }

    let alpha = image.alpha.as_ref().map(|values| {
        let mut cropped = Vec::with_capacity((width as usize) * (height as usize));
        for y in 0..height {
            let row = ((top + y) as usize) * (image.width as usize);
            let start = row + left as usize;
            cropped.extend_from_slice(&values[start..start + width as usize]);
        }
        cropped
    });

    Ok(DecodedImage {
        width,
        height,
        rgb,
        alpha,
        icc_profile: image.icc_profile.clone(),
        applied_orientation: image.applied_orientation,
    })
}

/// Mascara radial suave: vale 1 dentro de `inner` y cae a 0 en el borde.
///
/// `inner` es una fraccion del **semilado**, no de la diagonal: asi el radio llega
/// a 1 justo en el centro de cada borde del recorte (y a 1,41 en las esquinas), que
/// es lo que garantiza que la mascara valga 0 en todo el contorno. Normalizando
/// por la diagonal, el radio maximo fuera del centro apenas llega a 0,98 en las
/// esquinas y la mascara se quedaba a plena intensidad en medio de los bordes: ahi
/// el pegado tendria un corte duro, y un corte duro en el borde de un recorte se ve
/// como una costura.
///
/// El ultimo tramo no es lineal: se suaviza con `smoothstep`, porque una rampa
/// recta deja un borde de la mezcla visible —un anillo de un pixel— justo donde el
/// usuario mira, que es la cara.
pub fn radial_mask(width: u32, height: u32, inner: f32) -> Vec<f32> {
    let inner = inner.clamp(0.0, 0.999);
    let center_x = (width as f32 - 1.0) / 2.0;
    let center_y = (height as f32 - 1.0) / 2.0;
    let radius = width.min(height) as f32 / 2.0;
    if radius <= 0.0 {
        return Vec::new();
    }

    let mut mask = Vec::with_capacity((width as usize) * (height as usize));
    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 - center_x;
            let dy = y as f32 - center_y;
            let r = dx.hypot(dy) / radius;

            let value = if r <= inner {
                1.0
            } else if r >= 1.0 {
                0.0
            } else {
                let t = (r - inner) / (1.0 - inner);
                // smoothstep: 3t^2 - 2t^3
                1.0 - (t * t * (3.0 - 2.0 * t))
            };

            mask.push(value);
        }
    }

    mask
}

/// Pega `patch` sobre `base` en (`left`, `top`) mezclando con `mask` y un peso.
///
/// La mezcla es por pixel: `base = base * (1 - m) + patch * m`, con
/// `m = mask * weight` recortado a `0..1`. El canal alfa de la base **no se toca**:
/// la mascara describe cuanto color se sustituye, no la forma de la imagen.
///
/// Se exige que el parche y la mascara midan lo mismo: pegar una mascara de otra
/// medida no es "algo peor", es un resultado desalineado que nadie detectaria
/// mirandolo.
pub fn paste_masked(
    base: &mut DecodedImage,
    patch: &DecodedImage,
    left: u32,
    top: u32,
    mask: &[f32],
    weight: f32,
) -> SuResult<()> {
    let pixels = (patch.width as usize) * (patch.height as usize);
    if mask.len() != pixels {
        return Err(SuError::Internal(format!(
            "la mascara tiene {} valores y el parche {pixels} pixeles",
            mask.len()
        )));
    }

    if left + patch.width > base.width || top + patch.height > base.height {
        return Err(SuError::Internal(format!(
            "el parche {}x{} en ({left}, {top}) se sale de la imagen {}x{}",
            patch.width, patch.height, base.width, base.height
        )));
    }

    let weight = weight.clamp(0.0, 1.0);
    let stride = (base.width as usize) * 3;

    for y in 0..patch.height {
        for x in 0..patch.width {
            let m = (mask[(y as usize) * (patch.width as usize) + x as usize] * weight).clamp(0.0, 1.0);
            if m <= 0.0 {
                continue;
            }

            let target = (top + y) as usize * stride + (left + x) as usize * 3;
            let source = (y as usize) * (patch.width as usize) * 3 + (x as usize) * 3;

            for channel in 0..3 {
                let base_value = base.rgb[target + channel];
                let patch_value = patch.rgb[source + channel];
                base.rgb[target + channel] = base_value + (patch_value - base_value) * m;
            }
        }
    }

    Ok(())
}

/// Valida que una imagen de salida es plausible antes de escribirla.
///
/// Desviacion tipica de la luma (Rec. 709) en la escala `0..255`.
///
/// Es la medida que decide si una imagen es "plana". Se expone aparte de
/// [`validate_output`] porque es una propiedad medible por si sola, y asi la
/// prueba puede comparar el numero con el que deberia salir en lugar de limitarse
/// a comprobar que la validacion no falla.
pub fn luma_std_dev(image: &DecodedImage) -> f64 {
    let count = image.rgb.len() as f64 / 3.0;
    if count <= 0.0 {
        return 0.0;
    }

    let mut sum = 0.0f64;
    let mut sum_squares = 0.0f64;

    for pixel in image.rgb.chunks_exact(3) {
        let luma = (LUMA_RED * pixel[0] + LUMA_GREEN * pixel[1] + LUMA_BLUE * pixel[2]) as f64;
        sum += luma;
        sum_squares += luma * luma;
    }

    let mean = sum / count;
    let variance = (sum_squares / count) - (mean * mean);
    variance.max(0.0).sqrt() * 255.0
}

/// Detecta los tres fallos que de verdad ocurren: buffer uniforme (el modelo
/// devolvio negro o un color plano), valores no finitos (conversion a `fp16` mal
/// hecha) y rango dinamico colapsado.
///
/// La uniformidad se mide sobre la **luma** (Rec. 709), que es lo que percibe el
/// ojo y lo unico que puede llamarse con propiedad "imagen plana". El minimo, el
/// maximo y el rango siguen midiendose sobre los canales, porque un rango
/// colapsado es un problema de cuantizacion del buffer, no de brillo.
pub fn validate_output(image: &DecodedImage) -> SuResult<()> {
    let expected = image.pixel_count() * 3;
    if image.rgb.len() != expected {
        return Err(SuError::OutputValidationFailed {
            reason: format!(
                "el buffer tiene {} valores y deberia tener {expected}",
                image.rgb.len()
            ),
        });
    }

    let mut min = f32::MAX;
    let mut max = f32::MIN;

    for (pixel_index, pixel) in image.rgb.chunks_exact(3).enumerate() {
        for (channel, value) in pixel.iter().enumerate() {
            if !value.is_finite() {
                return Err(SuError::OutputValidationFailed {
                    reason: format!(
                        "valor no finito ({value}) en la posicion {}",
                        pixel_index * 3 + channel
                    ),
                });
            }
            min = min.min(*value);
            max = max.max(*value);
        }
    }

    let std_dev = luma_std_dev(image);

    if std_dev < MIN_LUMA_STD_DEV as f64 {
        return Err(SuError::OutputValidationFailed {
            reason: format!(
                "la imagen es uniforme (desviacion tipica de luma {std_dev:.3}/255)"
            ),
        });
    }

    // Un rango de 0 a 1 con un unico valor extremo es normal; lo que no lo es es
    // que toda la imagen se concentre en un margen minusculo.
    if (max - min) < 1.0 / 255.0 {
        return Err(SuError::OutputValidationFailed {
            reason: format!("rango dinamico colapsado ({min}..{max})"),
        });
    }

    Ok(())
}

/// Escribe la imagen de forma atomica: temporal, `fsync`, renombrado.
///
/// Si algo falla a mitad, el archivo destino no existe o sigue siendo el
/// anterior. Nunca queda un archivo truncado con el nombre definitivo.
pub fn write_atomic(
    image: &DecodedImage,
    path: &Path,
    format: OutputFormat,
    quality: u8,
) -> SuResult<()> {
    validate_output(image)?;

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| {
        SuError::WriteFailed(format!("no se pudo crear {}: {error}", parent.display()))
    })?;

    let temporary = temp_path_for(path);

    // El temporal se limpia pase lo que pase: un fallo no debe dejar basura en la
    // carpeta de salida del usuario.
    let result = write_image(image, &temporary, format, quality)
        .and_then(|()| sync_file(&temporary))
        .and_then(|()| {
            std::fs::rename(&temporary, path).map_err(|error| {
                SuError::WriteFailed(format!("no se pudo renombrar a {}: {error}", path.display()))
            })
        });

    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }

    result
}

/// Ruta del temporal de escritura.
///
/// Lleva el pid y un contador ademas del nombre del destino porque **dos trabajos
/// pueden escribir el mismo archivo a la vez**: al subir el limite de concurrencia,
/// dos imagenes identicas del mismo lote apuntan al mismo destino. Con un temporal
/// de nombre fijo, la segunda escritura pisaba el archivo de la primera antes de
/// que esta lo renombrara y el trabajo terminaba en fallo sin que nada estuviera
/// roto. El renombrado sigue siendo atomico: el destino se escribe entero de una
/// vez, y con dos escritores gana el ultimo, que es lo que se espera al procesar
/// dos veces la misma imagen.
fn temp_path_for(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(
        ".{}.{}{}",
        std::process::id(),
        TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        TEMP_SUFFIX
    ));
    path.with_file_name(name)
}

fn sync_file(path: &Path) -> SuResult<()> {
    let file = File::open(path)
        .map_err(|error| SuError::WriteFailed(format!("no se pudo reabrir el temporal: {error}")))?;
    file.sync_all()
        .map_err(|error| SuError::WriteFailed(format!("no se pudo sincronizar el temporal: {error}")))
}

fn write_image(
    image: &DecodedImage,
    path: &Path,
    format: OutputFormat,
    quality: u8,
) -> SuResult<()> {
    let dynamic = image.to_dynamic_image()?;

    match format {
        OutputFormat::Png => dynamic
            .save_with_format(path, ImageFormat::Png)
            .map_err(|error| SuError::WriteFailed(format!("PNG: {error}"))),

        // JPEG no admite canal alfa: se descarta en lugar de fallar. Avisar de
        // esto es responsabilidad de la UI, no de la capa de E/S.
        OutputFormat::Jpg => {
            let rgb = DynamicImage::ImageRgb8(dynamic.to_rgb8());
            let file = File::create(path)
                .map_err(|error| SuError::WriteFailed(format!("JPEG: {error}")))?;
            let mut writer = BufWriter::new(file);
            let encoder = JpegEncoder::new_with_quality(&mut writer, quality.clamp(1, 100));
            rgb.write_with_encoder(encoder)
                .map_err(|error| SuError::WriteFailed(format!("JPEG: {error}")))?;
            writer
                .flush()
                .map_err(|error| SuError::WriteFailed(format!("JPEG: {error}")))
        }

        // WebP se escribe sin perdida: el codificador incluido en `image` no
        // ofrece modo con calidad. El WebP con perdida llega en la Fase 3.
        OutputFormat::Webp => dynamic
            .save_with_format(path, ImageFormat::WebP)
            .map_err(|error| SuError::WriteFailed(format!("WebP: {error}"))),
    }
}

/// Ruta de salida a partir del archivo de origen, el sufijo y el formato.
///
/// El nombre se conserva para que el usuario reconozca el resultado; solo se
/// anade el sufijo y, si hace falta, se cambia la extension.
pub fn output_path(source: &Path, output_dir: &Path, suffix: &str, format: OutputFormat) -> PathBuf {
    let stem = source
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "imagen".to_string());

    output_dir.join(format!("{stem}{suffix}.{}", format.extension()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("su-imageio-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("directorio temporal");
        dir
    }

    /// Imagen con un degradado real: supera la validacion de salida.
    fn gradient(width: u32, height: u32) -> DecodedImage {
        let mut image = DecodedImage {
            width,
            height,
            rgb: Vec::with_capacity((width as usize) * (height as usize) * 3),
            alpha: None,
            icc_profile: None,
            applied_orientation: 1,
        };
        for y in 0..height {
            for x in 0..width {
                image.rgb.push(x as f32 / width as f32);
                image.rgb.push(y as f32 / height as f32);
                image.rgb.push(0.5);
            }
        }
        image
    }

    fn flat(width: u32, height: u32, value: f32) -> DecodedImage {
        let mut image = gradient(width, height);
        image.rgb.fill(value);
        image
    }

    /// Imagen de un solo color, con los tres canales independientes.
    fn flat_color(width: u32, height: u32, color: [f32; 3]) -> DecodedImage {
        let mut image = flat(width, height, 0.0);
        for pixel in image.rgb.chunks_exact_mut(3) {
            pixel.copy_from_slice(&color);
        }
        image
    }

    /// Imagen cuyo unico canal que varia es el azul.
    fn blue_only_gradient(width: u32, height: u32) -> DecodedImage {
        let mut image = flat(width, height, 0.0);
        for (index, pixel) in image.rgb.chunks_exact_mut(3).enumerate() {
            pixel[0] = 0.0;
            pixel[1] = 0.0;
            pixel[2] = index as f32 / (width as usize * height as usize) as f32;
        }
        image
    }

    #[test]
    fn orientation_6_swaps_the_dimensions() {
        let source = DynamicImage::ImageRgb8(RgbImage::from_pixel(40, 20, Rgb([10, 20, 30])));
        let rotated = apply_orientation(source, 6);
        assert_eq!(rotated.dimensions(), (20, 40));
    }

    #[test]
    fn orientation_8_swaps_the_dimensions_too() {
        let source = DynamicImage::ImageRgb8(RgbImage::from_pixel(40, 20, Rgb([10, 20, 30])));
        let rotated = apply_orientation(source, 8);
        assert_eq!(rotated.dimensions(), (20, 40));
    }

    #[test]
    fn orientation_1_and_unknown_values_leave_the_image_alone() {
        for orientation in [0u16, 1, 9, 255] {
            let source = DynamicImage::ImageRgb8(RgbImage::from_pixel(40, 20, Rgb([10, 20, 30])));
            let result = apply_orientation(source, orientation);
            assert_eq!(result.dimensions(), (40, 20), "orientacion {orientation}");
        }
    }

    #[test]
    fn orientation_3_keeps_the_size() {
        let source = DynamicImage::ImageRgb8(RgbImage::from_pixel(40, 20, Rgb([10, 20, 30])));
        assert_eq!(apply_orientation(source, 3).dimensions(), (40, 20));
    }

    #[test]
    fn orientation_6_rotates_the_content_clockwise() {
        // Una imagen 3x1 con los canales rojo, verde y azul de izquierda a
        // derecha; tras un giro de 90 grados deben quedar de arriba abajo.
        let mut source = RgbImage::new(3, 1);
        source.put_pixel(0, 0, Rgb([255, 0, 0]));
        source.put_pixel(1, 0, Rgb([0, 255, 0]));
        source.put_pixel(2, 0, Rgb([0, 0, 255]));

        let rotated = apply_orientation(DynamicImage::ImageRgb8(source), 6);
        assert_eq!(rotated.dimensions(), (1, 3));

        let rgb = rotated.to_rgb8();
        assert_eq!(rgb.get_pixel(0, 0)[0], 255, "el rojo deberia quedar arriba");
        assert_eq!(rgb.get_pixel(0, 2)[2], 255, "el azul deberia quedar abajo");
    }

    #[test]
    fn dynamic_image_round_trip_preserves_pixels() {
        let original = DynamicImage::ImageRgb8(RgbImage::from_fn(7, 5, |x, y| {
            Rgb([(x * 20) as u8, (y * 30) as u8, 128])
        }));

        let decoded = from_dynamic_image(&original);
        assert_eq!(decoded.width, 7);
        assert_eq!(decoded.height, 5);
        assert!(!decoded.has_alpha());

        let restored = decoded.to_dynamic_image().expect("reconstruccion");
        assert_eq!(
            restored.to_rgb8().into_raw(),
            original.to_rgb8().into_raw()
        );
    }

    #[test]
    fn alpha_channel_is_kept_separate() {
        let source = DynamicImage::ImageRgba8(RgbaImage::from_fn(4, 4, |x, _| {
            image::Rgba([10, 20, 30, (x * 60) as u8])
        }));

        let decoded = from_dynamic_image(&source);
        assert!(decoded.has_alpha());
        assert_eq!(decoded.rgb.len(), 16 * 3);
        assert_eq!(decoded.alpha.as_ref().map(Vec::len), Some(16));
    }

    #[test]
    fn alpha_round_trips() {
        let source = DynamicImage::ImageRgba8(RgbaImage::from_fn(4, 4, |x, _| {
            image::Rgba([10, 20, 30, (x * 60) as u8])
        }));
        let restored = from_dynamic_image(&source).to_dynamic_image().unwrap();
        assert_eq!(restored.to_rgba8().into_raw(), source.to_rgba8().into_raw());
    }

    /// Imagen 8x8: una franja visible a la izquierda (x < 4) y el resto
    /// transparente con un color de fondo que no tiene nada que ver.
    fn half_transparent(background: [u8; 3]) -> DecodedImage {
        let mut image = DecodedImage {
            width: 8,
            height: 8,
            rgb: Vec::with_capacity(8 * 8 * 3),
            alpha: Some(Vec::with_capacity(8 * 8)),
            icc_profile: None,
            applied_orientation: 1,
        };

        for _ in 0..8 {
            for x in 0..8u32 {
                if x < 4 {
                    image.rgb.extend([0.0, 0.0, 0.0]);
                    image.alpha.as_mut().unwrap().push(1.0);
                } else {
                    image
                        .rgb
                        .extend(background.map(|value| value as f32 / 255.0));
                    image.alpha.as_mut().unwrap().push(0.0);
                }
            }
        }

        image
    }

    fn pixel(image: &DecodedImage, x: u32, y: u32) -> [f32; 3] {
        let index = ((y * image.width + x) * 3) as usize;
        [
            image.rgb[index],
            image.rgb[index + 1],
            image.rgb[index + 2],
        ]
    }

    #[test]
    fn a_transparent_region_takes_the_colour_of_the_nearest_visible_pixel() {
        // El color de fondo (magenta) no puede sobrevivir: el modelo reconstruiria
        // un contorno falso entre el y el dibujo.
        let source = half_transparent([255, 0, 255]);
        let bled = bleed_transparent(&source);
        assert!(
            matches!(bled, Cow::Owned(_)),
            "con transparencia de verdad hay que construir la imagen"
        );

        for x in 0..8 {
            assert_eq!(pixel(&bled, x, 3), [0.0, 0.0, 0.0], "columna {x}");
        }
    }

    #[test]
    fn bleeding_leaves_the_alpha_channel_and_the_size_alone() {
        let source = half_transparent([255, 0, 255]);
        let bled = bleed_transparent(&source);

        assert_eq!((bled.width, bled.height), (source.width, source.height));
        assert_eq!(bled.alpha, source.alpha, "el alfa es la forma de la imagen");
    }

    #[test]
    fn a_semi_transparent_pixel_keeps_its_own_colour() {
        // Un pixel con alfa distinto de cero si aporta color a la mezcla: cambiarlo
        // cambiaria la imagen que ve el usuario, y esto es solo un relleno de
        // zonas invisibles.
        let mut source = half_transparent([255, 0, 255]);
        let index = (2 * 8 + 5) as usize;
        source.rgb[index * 3] = 0.25;
        source.alpha.as_mut().unwrap()[index] = 0.4;

        let bled = bleed_transparent(&source);
        assert_eq!(pixel(&bled, 5, 2)[0], 0.25);
    }

    #[test]
    fn bleeding_without_alpha_changes_nothing() {
        let source = gradient(16, 16);
        assert!(!source.has_alpha());
        // Prestada, no copiada: es el caso de todas las fotos.
        assert!(matches!(bleed_transparent(&source), Cow::Borrowed(_)));
        assert_eq!(bleed_transparent(&source).as_ref(), &source);
    }

    #[test]
    fn an_opaque_png_is_not_copied_either() {
        // Un PNG guardado con canal alfa pero sin ningun pixel transparente: real y
        // frecuente. No hay nada que rellenar, asi que no se toca el RGB.
        let mut source = gradient(16, 16);
        source.alpha = Some(vec![1.0; 256]);

        assert!(
            matches!(bleed_transparent(&source), Cow::Borrowed(_)),
            "un alfa entero a 1 no necesita relleno ni copia"
        );
    }

    #[test]
    fn an_image_that_is_all_transparent_is_returned_alone() {
        // Sin ningun pixel visible no hay color que extender. No es un caso de
        // laboratorio: es lo que llega si el usuario elige un PNG recortado vacio.
        let mut source = gradient(8, 8);
        source.alpha = Some(vec![0.0; 64]);

        assert!(matches!(bleed_transparent(&source), Cow::Borrowed(_)));
        assert_eq!(bleed_transparent(&source).as_ref(), &source);
    }

    #[test]
    fn a_malformed_buffer_is_rejected_when_rebuilding() {
        let broken = DecodedImage {
            width: 4,
            height: 4,
            rgb: vec![0.0; 10],
            alpha: None,
            icc_profile: None,
            applied_orientation: 1,
        };
        assert!(broken.to_dynamic_image().is_err());
    }

    #[test]
    fn crop_takes_the_requested_region_and_keeps_the_alpha() {
        let mut image = gradient(16, 16);
        image.alpha = Some(vec![0.25; 16 * 16]);

        let cut = crop(&image, 4, 8, 6, 4).expect("recorte");
        assert_eq!((cut.width, cut.height), (6, 4));
        assert_eq!(cut.rgb.len(), 6 * 4 * 3);
        assert_eq!(cut.alpha.as_ref().map(|a| a.len()), Some(6 * 4));

        // El primer pixel del recorte es el (4, 8) de la original.
        let original = ((8 * 16) + 4) * 3;
        assert_eq!(cut.rgb[0], image.rgb[original]);
        assert_eq!(cut.rgb[1], image.rgb[original + 1]);

        // Y el ultimo, el (9, 11).
        let last = ((11 * 16) + 9) * 3;
        assert_eq!(cut.rgb[cut.rgb.len() - 3], image.rgb[last]);
    }

    #[test]
    fn a_crop_that_does_not_fit_is_refused() {
        let image = gradient(16, 16);
        assert!(crop(&image, 8, 8, 9, 9).is_err());
        assert!(crop(&image, 0, 0, 0, 4).is_err());
    }

    #[test]
    fn the_radial_mask_is_full_inside_and_zero_at_the_border() {
        let mask = radial_mask(65, 65, 0.6);
        let side = 65usize;

        let center = mask[32 * side + 32];
        assert_eq!(center, 1.0, "el centro tiene que ser plena intensidad");

        // El contorno entero queda practicamente a cero: esquinas y puntos medios de
        // los bordes. En un recorte de lado impar el pixel del borde no llega a la
        // distancia teorica exacta, asi que se admite una milesima.
        for index in [0, side - 1, (side - 1) * side, side * side - 1, 32, 32 * side + 64] {
            assert!(
                mask[index] < 0.01,
                "la mascara tiene que anularse en el contorno (posicion {index}: {})",
                mask[index]
            );
        }

        // Propiedad, no "no lanza": a lo largo de un radio la mascara no crece al
        // alejarse del centro, y la transicion esta donde dice `inner`.
        let mut previous = 1.0;
        for distance in 0..32 {
            let value = mask[32 * side + (32 + distance)];
            assert!(value <= previous, "la mascara crecio al alejarse del centro");
            previous = value;
        }

        let transition = mask[32 * side + (32 + 30)];
        assert!(
            (0.0..1.0).contains(&transition),
            "tiene que haber transicion antes del borde: {transition}"
        );
    }

    #[test]
    fn paste_masked_only_touches_what_the_mask_covers() {
        let mut base = flat(8, 8, 0.0);
        let patch = flat(4, 4, 1.0);
        let mask = radial_mask(4, 4, 0.5);

        paste_masked(&mut base, &patch, 2, 2, &mask, 1.0).expect("pegado");

        // El centro del parche esta a plena intensidad...
        let center = ((4 * 8) + 4) * 3;
        assert!(base.rgb[center] > 0.9, "centro: {}", base.rgb[center]);

        // ...y fuera del parche no se ha tocado nada.
        assert_eq!(base.rgb[0], 0.0);
        let just_outside = (8 + 1) * 3;
        assert_eq!(base.rgb[just_outside], 0.0);
    }

    #[test]
    fn paste_masked_scales_the_mask_with_the_weight() {
        let patch = flat(4, 4, 1.0);
        // Mascara plana a 1: asi el resultado es el peso exacto y la prueba mide el
        // peso, no la forma de la mascara radial (que tiene su propia prueba).
        let mask = vec![1.0f32; 16];

        let mut half = flat(4, 4, 0.0);
        paste_masked(&mut half, &patch, 0, 0, &mask, 0.5).expect("pegado");
        assert!((half.rgb[0] - 0.5).abs() < 1e-6, "peso 0.5: {}", half.rgb[0]);

        let mut none = flat(4, 4, 0.0);
        paste_masked(&mut none, &patch, 0, 0, &mask, 0.0).expect("pegado");
        assert_eq!(none.rgb[0], 0.0, "peso 0: la base no cambia");
    }

    #[test]
    fn a_mismatched_mask_or_patch_is_refused() {
        let mut base = flat(8, 8, 0.0);
        let patch = flat(4, 4, 1.0);

        assert!(paste_masked(&mut base, &patch, 0, 0, &[1.0; 3], 1.0).is_err());
        assert!(paste_masked(&mut base, &patch, 6, 6, &[1.0; 16], 1.0).is_err());
    }

    #[test]
    fn validation_accepts_a_real_image() {
        assert!(validate_output(&gradient(64, 64)).is_ok());
    }

    #[test]
    fn validation_rejects_a_uniform_buffer() {
        // Este es el fallo clasico: el modelo devuelve negro y la herramienta lo
        // guarda como si fuera un resultado valido.
        let error = validate_output(&flat(32, 32, 0.0)).unwrap_err();
        assert_eq!(error.code().as_str(), "SU-E141");
        assert!(matches!(error, SuError::OutputValidationFailed { .. }));
    }

    #[test]
    fn validation_rejects_a_uniform_saturated_color() {
        // BUG-09: la comprobacion de uniformidad media los tres canales mezclados,
        // asi que un color plano saturado pasaba el filtro. Un rojo uniforme
        // tiene una desviacion de 121/255 mezclando canales, pero su luma es una
        // sola: la imagen es tan plana como una negra.
        for color in [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]] {
            let error = validate_output(&flat_color(16, 16, color)).unwrap_err();
            assert!(
                matches!(error, SuError::OutputValidationFailed { .. }),
                "un color plano {color:?} deberia rechazarse"
            );
        }
    }

    #[test]
    fn validation_accepts_an_image_that_only_varies_in_one_channel() {
        // El otro lado de la moneda: medir luma no puede rechazar una imagen
        // valida. Un degradado que solo mueve el azul apenas cambia de luma
        // (0.0722 del rango), y sigue muy por encima del minimo.
        assert!(validate_output(&blue_only_gradient(64, 64)).is_ok());
    }

    #[test]
    fn the_uniformity_check_measures_rec709_luma() {
        // Propiedad, no "no lanza": una imagen de dos mitades que difieren solo en
        // el canal rojo en 0.4 tiene que dar una desviacion de luma de
        // 0.2126 * 0.4 * 255 / 2 = 10.84. Se comprueba con las constantes del
        // propio codigo para que la prueba no repita los coeficientes a mano.
        let mut image = flat_color(32, 32, [0.0, 0.5, 0.5]);
        let half = image.rgb.len() / 2;
        for pixel in image.rgb[..half].chunks_exact_mut(3) {
            pixel[0] = 0.4;
        }

        let expected_std = (LUMA_RED * 0.4) as f64 * 255.0 / 2.0;
        assert!(expected_std > MIN_LUMA_STD_DEV as f64);

        let measured = luma_std_dev(&image);
        assert!(
            (measured - expected_std).abs() < 0.01,
            "desviacion medida {measured}, esperada {expected_std}"
        );
    }

    #[test]
    fn validation_rejects_non_finite_values() {
        let mut image = gradient(32, 32);
        image.rgb[100] = f32::NAN;
        assert!(validate_output(&image).is_err());

        let mut infinite = gradient(32, 32);
        infinite.rgb[50] = f32::INFINITY;
        assert!(validate_output(&infinite).is_err());
    }

    #[test]
    fn validation_rejects_a_collapsed_dynamic_range() {
        let mut image = gradient(32, 32);
        image.rgb.fill(0.5);
        // Todos los valores a 0.5 menos uno, para que la desviacion no sea cero
        // pero el rango si sea minusculo.
        image.rgb[0] = 0.5001;
        assert!(validate_output(&image).is_err());
    }

    #[test]
    fn validation_rejects_a_buffer_of_the_wrong_length() {
        let mut image = gradient(32, 32);
        image.rgb.truncate(10);
        assert!(validate_output(&image).is_err());
    }

    #[test]
    fn resize_produces_the_requested_size() {
        let source = gradient(64, 32);
        let doubled =
            resize(&source, 128, 64, ResizeKernel::Lanczos3).expect("reescalado");
        assert_eq!((doubled.width, doubled.height), (128, 64));
        assert_eq!(doubled.rgb.len(), 128 * 64 * 3);
    }

    #[test]
    fn resize_to_zero_is_rejected() {
        let source = gradient(16, 16);
        assert!(resize(&source, 0, 10, ResizeKernel::Lanczos3).is_err());
        assert!(resize(&source, 10, 0, ResizeKernel::Lanczos3).is_err());
    }

    #[test]
    fn every_kernel_keeps_the_data_in_range() {
        // Una tabla de interpolacion mal construida produce sobrepasos (valores
        // negativos o por encima de 1) que se convierten en manchas al guardar.
        let source = gradient(32, 32);

        for kernel in [
            ResizeKernel::Lanczos3,
            ResizeKernel::CatmullRom,
            ResizeKernel::Gaussian,
            ResizeKernel::Triangle,
            ResizeKernel::Nearest,
        ] {
            let upscaled = resize(&source, 96, 96, kernel).expect("ampliacion");
            for value in &upscaled.rgb {
                assert!(
                    (-0.001..=1.001).contains(value),
                    "{} dejo un valor fuera de rango: {value}",
                    kernel.as_str()
                );
            }
        }
    }

    #[test]
    fn a_lanczos_halving_preserves_a_flat_region_exactly() {
        // Comprueba que el reescalado no cuantiza: con 8 bits, una region plana de
        // valor 0.5 volveria como 127/255 = 0.498.
        let source = flat(64, 64, 0.5);
        let halved = resize(&source, 32, 32, ResizeKernel::Lanczos3).expect("reduccion");
        for value in &halved.rgb {
            assert!(
                (value - 0.5).abs() < 1e-6,
                "la reduccion perdio el valor exacto: {value}"
            );
        }
    }

    #[test]
    fn resizing_keeps_the_icc_profile_and_the_orientation() {
        let mut source = gradient(16, 16);
        source.icc_profile = Some(vec![1, 2, 3]);
        source.applied_orientation = 6;

        let doubled = resize(&source, 32, 32, ResizeKernel::Lanczos3).expect("reescalado");
        assert_eq!(doubled.icc_profile.as_deref(), Some(&[1u8, 2, 3][..]));
        assert_eq!(doubled.applied_orientation, 6);
    }

    #[test]
    fn resizing_to_the_same_size_returns_the_same_pixels() {
        let source = gradient(24, 24);
        let same = resize(&source, 24, 24, ResizeKernel::Lanczos3).expect("identidad");
        assert_eq!(same.rgb, source.rgb);
    }

    #[test]
    fn the_kernel_names_round_trip_and_unknown_ones_are_rejected() {
        for kernel in [
            ResizeKernel::Lanczos3,
            ResizeKernel::CatmullRom,
            ResizeKernel::Gaussian,
            ResizeKernel::Triangle,
            ResizeKernel::Nearest,
        ] {
            assert_eq!(ResizeKernel::parse(kernel.as_str()).unwrap(), kernel);
        }

        // Mayusculas y espacios no cambian el significado.
        assert_eq!(
            ResizeKernel::parse("  LANCZOS3 ").unwrap(),
            ResizeKernel::Lanczos3
        );
        // `bicubic` es el nombre popular de la familia de Catmull-Rom.
        assert_eq!(
            ResizeKernel::parse("bicubic").unwrap(),
            ResizeKernel::CatmullRom
        );
        assert!(ResizeKernel::parse("spline").is_err());
    }

    #[test]
    fn output_path_keeps_the_stem_and_changes_the_extension() {
        let path = output_path(
            Path::new("/fotos/retrato.jpeg"),
            Path::new("/salida"),
            "_upscaled",
            OutputFormat::Png,
        );
        assert_eq!(path, Path::new("/salida/retrato_upscaled.png"));
    }

    #[test]
    fn output_path_works_with_windows_separators() {
        let path = output_path(
            Path::new(r"C:\fotos\retrato.png"),
            Path::new(r"C:\salida"),
            "_x4",
            OutputFormat::Jpg,
        );
        assert!(path.to_string_lossy().ends_with("retrato_x4.jpg"), "{path:?}");
    }

    #[test]
    fn writing_is_atomic_and_leaves_no_temporary_behind() {
        let dir = scratch("atomic");
        let target = dir.join("salida.png");
        let image = gradient(64, 64);

        write_atomic(&image, &target, OutputFormat::Png, 95).expect("escritura");

        assert!(target.exists(), "no se creo el archivo final");
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .expect("lectura del directorio")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.ends_with(TEMP_SUFFIX))
            .collect();
        assert!(leftovers.is_empty(), "quedaron temporales: {leftovers:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_write_does_not_create_the_destination() {
        let dir = scratch("failed");
        let target = dir.join("salida.png");

        // Un buffer uniforme no pasa la validacion, asi que no debe escribirse nada.
        let error = write_atomic(&flat(32, 32, 0.0), &target, OutputFormat::Png, 95).unwrap_err();
        assert_eq!(error.code().as_str(), "SU-E141");
        assert!(!target.exists(), "se escribio un archivo que no era valido");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn writing_creates_the_output_directory() {
        let dir = scratch("mkdir");
        let nested = dir.join("a").join("b");
        let target = nested.join("salida.png");

        write_atomic(&gradient(32, 32), &target, OutputFormat::Png, 95).expect("escritura");
        assert!(target.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn decode_reports_a_missing_file_clearly() {
        let error = decode(Path::new("/no/existe/realmente.png")).unwrap_err();
        assert_eq!(error.code().as_str(), "SU-E161");
    }

    #[test]
    fn decode_rejects_a_file_that_is_not_an_image() {
        let dir = scratch("garbage");
        let path = dir.join("no-es-imagen.png");
        std::fs::write(&path, b"esto no es un PNG").expect("escritura");

        let error = decode(&path).unwrap_err();
        assert!(matches!(
            error.code().as_str(),
            "SU-E100" | "SU-E101" | "SU-E102"
        ));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_full_decode_and_write_cycle_works() {
        let dir = scratch("cycle");
        let source = dir.join("origen.png");
        let target = dir.join("destino.png");

        gradient(48, 32)
            .to_dynamic_image()
            .expect("conversion")
            .save_with_format(&source, ImageFormat::Png)
            .expect("guardado del origen");

        let decoded = decode(&source).expect("decodificacion");
        assert_eq!((decoded.width, decoded.height), (48, 32));
        assert_eq!(decoded.applied_orientation, 1);

        write_atomic(&decoded, &target, OutputFormat::Png, 95).expect("escritura");
        assert!(target.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_without_exif_reports_orientation_1() {
        let dir = scratch("noexif");
        let source = dir.join("plano.png");
        gradient(16, 16)
            .to_dynamic_image()
            .unwrap()
            .save_with_format(&source, ImageFormat::Png)
            .unwrap();

        assert_eq!(read_exif_orientation(&source), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
