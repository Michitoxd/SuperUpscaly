//! Planificacion de tiles y composicion sin costuras.
//!
//! ## Como se reparte la imagen
//!
//! La imagen se rellena hasta un multiplo de `pad_to` (necesario para que
//! TensorRT y DirectML trabajen con formas alineadas) y se trocea con un paso
//! `tile - overlap`. El **ultimo** tile de cada eje se alinea con el borde
//! derecho/inferior en lugar de dejar un resto diminuto: asi el solape real de
//! ese par es mayor que el nominal, pero el resto de pares conservan el solape
//! pedido y no se desperdicia trabajo.
//!
//! ## Por que no hay costuras
//!
//! Cada tile pondera sus pixeles con una ventana coseno elevada
//! (`0.5 * (1 - cos(pi * (k + 0.5) / ov))`). En una zona de solape de anchura
//! `ov`, la rampa de bajada del tile izquierdo y la de subida del derecho suman
//! exactamente 1:
//!
//! ```text
//!   wA(x) + wB(x) = 1 - 0.5 * [ cos(pi - a) + cos(a) ] = 1,  con a = pi*(k+0.5)/ov
//! ```
//!
//! Por eso el compositor acumula `valor * peso` y divide por la suma de pesos:
//! aunque el modelo devuelva valores distintos para el mismo pixel en dos tiles,
//! la transicion es un degradado continuo en lugar de un escalon visible.

use serde::{Deserialize, Serialize};
use su_core::{SuError, SuResult};

/// Tamano minimo de tile. Por debajo de esto la inferencia se vuelve dominada
/// por el coste fijo de cada pasada.
pub const MIN_TILE: u32 = 64;
/// Limite defensivo: un plan con mas tiles que esto es un error de calculo.
pub const MAX_TILES: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tile {
    pub row: u32,
    pub col: u32,
    /// Origen del tile dentro del lienzo rellenado (en pixeles de entrada).
    pub canvas_x: u32,
    pub canvas_y: u32,
    /// Cuantos pixeles de entrada lee este tile.
    pub read_w: u32,
    pub read_h: u32,
    /// Solapes reales con los tiles vecinos, en pixeles de entrada.
    pub overlap_left: u32,
    pub overlap_top: u32,
    pub overlap_right: u32,
    pub overlap_bottom: u32,
}

impl Tile {
    /// Cuantos pixeles de salida produce este tile.
    pub fn output_size(&self, scale: u32) -> (u32, u32) {
        (self.read_w * scale, self.read_h * scale)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TilePlan {
    pub image_width: u32,
    pub image_height: u32,
    /// Dimensiones tras rellenar hasta un multiplo de `pad_to`.
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub tile: u32,
    pub overlap: u32,
    pub pad_to: u32,
    pub starts_x: Vec<u32>,
    pub starts_y: Vec<u32>,
    /// `true` si la imagen cabe en un solo tile: no se trocea ni se compone.
    pub single_pass: bool,
}

impl TilePlan {
    pub fn new(image_width: u32, image_height: u32, tile: u32, overlap: u32, pad_to: u32) -> SuResult<Self> {
        if image_width == 0 || image_height == 0 {
            return Err(SuError::Internal(
                "no se puede planificar el tiling de una imagen vacia".to_string(),
            ));
        }
        if tile < MIN_TILE {
            return Err(SuError::Internal(format!(
                "tile {tile} por debajo del minimo {MIN_TILE}"
            )));
        }
        let pad_to = pad_to.max(1);
        let canvas_width = round_up(image_width, pad_to);
        let canvas_height = round_up(image_height, pad_to);

        let single_pass = canvas_width <= tile && canvas_height <= tile;

        let (starts_x, starts_y) = if single_pass {
            (vec![0], vec![0])
        } else {
            (
                axis_starts(canvas_width, tile, overlap),
                axis_starts(canvas_height, tile, overlap),
            )
        };

        let count = starts_x.len().saturating_mul(starts_y.len());
        if count == 0 || count > MAX_TILES {
            return Err(SuError::Internal(format!(
                "plan de tiling absurdo: {count} tiles para {image_width}x{image_height}"
            )));
        }

        Ok(Self {
            image_width,
            image_height,
            canvas_width,
            canvas_height,
            tile,
            overlap,
            pad_to,
            starts_x,
            starts_y,
            single_pass,
        })
    }

    pub fn cols(&self) -> u32 {
        self.starts_x.len() as u32
    }

    pub fn rows(&self) -> u32 {
        self.starts_y.len() as u32
    }

    pub fn count(&self) -> u32 {
        self.cols() * self.rows()
    }

    /// Solape con el tile anterior de un eje, o 0 si es el primero.
    fn overlap_before(starts: &[u32], tile: u32, index: usize) -> u32 {
        if index == 0 {
            return 0;
        }
        let previous = starts[index - 1];
        let current = starts[index];
        (previous + tile).saturating_sub(current)
    }

    /// Solape con el tile siguiente de un eje, o 0 si es el ultimo.
    fn overlap_after(starts: &[u32], tile: u32, index: usize) -> u32 {
        if index + 1 >= starts.len() {
            return 0;
        }
        let current = starts[index];
        let next = starts[index + 1];
        (current + tile).saturating_sub(next)
    }

    /// Lista de tiles en orden fila a fila.
    pub fn tiles(&self) -> Vec<Tile> {
        let mut tiles = Vec::with_capacity(self.count() as usize);
        for (row_index, &canvas_y) in self.starts_y.iter().enumerate() {
            for (col_index, &canvas_x) in self.starts_x.iter().enumerate() {
                tiles.push(Tile {
                    row: row_index as u32,
                    col: col_index as u32,
                    canvas_x,
                    canvas_y,
                    read_w: self.tile.min(self.canvas_width - canvas_x),
                    read_h: self.tile.min(self.canvas_height - canvas_y),
                    overlap_left: Self::overlap_before(&self.starts_x, self.tile, col_index),
                    overlap_top: Self::overlap_before(&self.starts_y, self.tile, row_index),
                    overlap_right: Self::overlap_after(&self.starts_x, self.tile, col_index),
                    overlap_bottom: Self::overlap_after(&self.starts_y, self.tile, row_index),
                });
            }
        }
        tiles
    }

    /// Dimensiones de salida esperadas para un factor de escala.
    pub fn output_size(&self, scale: u32) -> (u32, u32) {
        (self.image_width * scale, self.image_height * scale)
    }

    /// Cuantos pixeles del canvas caen fuera de la imagen real. Se usa para
    /// avisar en el log: un relleno grande desperdicia inferencia.
    pub fn padding_pixels(&self) -> (u32, u32) {
        (
            self.canvas_width - self.image_width,
            self.canvas_height - self.image_height,
        )
    }
}

fn round_up(value: u32, multiple: u32) -> u32 {
    if multiple <= 1 {
        return value;
    }
    value.div_ceil(multiple) * multiple
}

/// Origenes de los tiles en un eje. El ultimo se alinea al borde final.
fn axis_starts(canvas: u32, tile: u32, overlap: u32) -> Vec<u32> {
    if canvas <= tile {
        return vec![0];
    }

    // Un solape >= al tile dejaria paso cero y colgaria el bucle.
    let overlap = overlap.min(tile / 2);
    let stride = (tile - overlap).max(1);

    let mut starts = vec![0u32];
    let mut x: u32 = 0;

    while x + tile < canvas {
        x = x.saturating_add(stride);
        if x + tile >= canvas {
            let last = canvas - tile;
            if last > starts.last().copied().unwrap_or(0) {
                starts.push(last);
            }
            break;
        }
        starts.push(x);
        if starts.len() >= MAX_TILES {
            break;
        }
    }

    starts
}

/// Pesos de una dimension para un tile, en pixeles de **salida**.
///
/// Fuera de las zonas de solape vale 1. En una rampa de entrada sube de 0 a 1
/// con un coseno elevado; en una de salida baja de 1 a 0 con la misma curva.
pub fn axis_weights(len: u32, overlap_start: u32, overlap_end: u32) -> Vec<f32> {
    let mut weights = vec![1.0f32; len as usize];
    let len_f = len as f32;

    if overlap_start > 0 {
        let span = overlap_start as f32;
        let limit = overlap_start.min(len) as usize;
        for (x, weight) in weights.iter_mut().enumerate().take(limit) {
            *weight = ramp_up(x as f32 + 0.5, span);
        }
    }

    if overlap_end > 0 {
        let span = overlap_end as f32;
        let limit = overlap_end.min(len) as usize;
        for offset in 0..limit {
            let x = len as usize - 1 - offset;
            let position = len_f - (x as f32) - 0.5;
            weights[x] = ramp_up(position, span);
        }
    }

    weights
}

/// Rampa coseno elevada: 0 en `position = 0`, 1 en `position = span`.
fn ramp_up(position: f32, span: f32) -> f32 {
    let t = (position / span).clamp(0.0, 1.0);
    0.5 * (1.0 - (std::f32::consts::PI * t).cos())
}

/// Compone los tiles en la imagen final.
///
/// `read_tile` recibe un tile y devuelve sus pixeles ya escalados, en formato
/// interleaved de `channels` canales y en `f32`. El compositor no sabe nada de
/// ONNX: eso lo hace testeable con un generador sintetico.
///
/// Los pixeles del relleno (fuera de `image_width` x `image_height`) se
/// descartan, de modo que la salida mide exactamente
/// `image_width * scale` x `image_height * scale`.
pub fn composite<F>(
    plan: &TilePlan,
    scale: u32,
    channels: usize,
    mut read_tile: F,
) -> SuResult<Vec<f32>>
where
    F: FnMut(&Tile) -> SuResult<Vec<f32>>,
{
    if channels == 0 {
        return Err(SuError::Internal("canales = 0".to_string()));
    }
    if scale == 0 {
        return Err(SuError::Internal("escala = 0".to_string()));
    }

    let (out_width, out_height) = plan.output_size(scale);
    let pixel_count = (out_width as usize) * (out_height as usize);
    let mut accumulator = vec![0.0f32; pixel_count * channels];
    let mut weight_sum = vec![0.0f32; pixel_count];

    for tile in plan.tiles() {
        let (tile_out_w, tile_out_h) = tile.output_size(scale);
        let expected = (tile_out_w as usize) * (tile_out_h as usize) * channels;
        let data = read_tile(&tile)?;

        if data.len() != expected {
            return Err(SuError::TileFailed {
                row: tile.row,
                col: tile.col,
                reason: format!(
                    "el tile devolvio {} valores, se esperaban {expected}",
                    data.len()
                ),
            });
        }

        let weights_x = axis_weights(
            tile_out_w,
            tile.overlap_left * scale,
            tile.overlap_right * scale,
        );
        let weights_y = axis_weights(
            tile_out_h,
            tile.overlap_top * scale,
            tile.overlap_bottom * scale,
        );

        let origin_x = tile.canvas_x * scale;
        let origin_y = tile.canvas_y * scale;

        for local_y in 0..tile_out_h {
            let out_y = origin_y + local_y;
            if out_y >= out_height {
                continue;
            }
            let weight_y = weights_y[local_y as usize];
            let row_offset = (out_y as usize) * (out_width as usize);

            for local_x in 0..tile_out_w {
                let out_x = origin_x + local_x;
                if out_x >= out_width {
                    continue;
                }

                let weight = weight_y * weights_x[local_x as usize];
                let target = row_offset + out_x as usize;
                let source = ((local_y as usize) * (tile_out_w as usize) + local_x as usize) * channels;

                for channel in 0..channels {
                    accumulator[target * channels + channel] += data[source + channel] * weight;
                }
                weight_sum[target] += weight;
            }
        }
    }

    // Normalizacion. Si algun pixel se quedo sin cobertura es un fallo del plan,
    // no algo que se pueda ignorar: saldria negro y el usuario lo veria.
    for (index, total) in weight_sum.iter().enumerate() {
        if *total <= f32::EPSILON {
            let x = index as u32 % out_width;
            let y = index as u32 / out_width;
            return Err(SuError::Internal(format!(
                "el pixel {x},{y} no lo cubre ningun tile: plan de tiling invalido"
            )));
        }
    }

    for (index, total) in weight_sum.iter().enumerate() {
        for channel in 0..channels {
            accumulator[index * channels + channel] /= total;
        }
    }

    Ok(accumulator)
}

/// Suma de pesos por pixel. Se expone para poder verificar en tests que la
/// normalizacion es exacta sin tener que invertir el compositor.
pub fn coverage(plan: &TilePlan, scale: u32) -> SuResult<Vec<f32>> {
    let (out_width, out_height) = plan.output_size(scale);
    let mut weight_sum = vec![0.0f32; (out_width as usize) * (out_height as usize)];

    for tile in plan.tiles() {
        let (tile_out_w, tile_out_h) = tile.output_size(scale);
        let weights_x = axis_weights(
            tile_out_w,
            tile.overlap_left * scale,
            tile.overlap_right * scale,
        );
        let weights_y = axis_weights(
            tile_out_h,
            tile.overlap_top * scale,
            tile.overlap_bottom * scale,
        );

        let origin_x = tile.canvas_x * scale;
        let origin_y = tile.canvas_y * scale;

        for local_y in 0..tile_out_h {
            let out_y = origin_y + local_y;
            if out_y >= out_height {
                continue;
            }
            for local_x in 0..tile_out_w {
                let out_x = origin_x + local_x;
                if out_x >= out_width {
                    continue;
                }
                weight_sum[(out_y as usize) * (out_width as usize) + out_x as usize] +=
                    weights_y[local_y as usize] * weights_x[local_x as usize];
            }
        }
    }

    Ok(weight_sum)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(width: u32, height: u32, tile: u32, overlap: u32) -> TilePlan {
        TilePlan::new(width, height, tile, overlap, 32).expect("plan valido")
    }

    #[test]
    fn small_image_is_a_single_pass() {
        let plan = plan(400, 300, 512, 32);
        assert!(plan.single_pass);
        assert_eq!(plan.count(), 1);
        assert_eq!(plan.tiles()[0].read_w, 416); // 400 redondeado a multiplo de 32
    }

    #[test]
    fn canvas_is_padded_to_the_alignment() {
        let plan = plan(1000, 700, 512, 32);
        assert_eq!(plan.canvas_width, 1024);
        assert_eq!(plan.canvas_height, 704);
        assert_eq!(plan.padding_pixels(), (24, 4));
    }

    #[test]
    fn tiles_cover_the_whole_canvas_without_gaps() {
        let plan = plan(2048, 1536, 512, 32);
        let mut covered = vec![false; (plan.canvas_width as usize) * (plan.canvas_height as usize)];

        for tile in plan.tiles() {
            for y in tile.canvas_y..tile.canvas_y + tile.read_h {
                for x in tile.canvas_x..tile.canvas_x + tile.read_w {
                    covered[(y as usize) * (plan.canvas_width as usize) + x as usize] = true;
                }
            }
        }

        let holes = covered.iter().filter(|flag| !**flag).count();
        assert_eq!(holes, 0, "quedaron {holes} pixeles sin cubrir");
    }

    #[test]
    fn consecutive_tiles_overlap_by_at_least_the_requested_amount() {
        let plan = plan(2048, 1536, 512, 32);
        for tile in plan.tiles() {
            if tile.col + 1 < plan.cols() {
                assert!(
                    tile.overlap_right >= 32,
                    "solape derecho insuficiente: {}",
                    tile.overlap_right
                );
            }
            if tile.row + 1 < plan.rows() {
                assert!(tile.overlap_bottom >= 32);
            }
        }
    }

    #[test]
    fn overlap_is_symmetric_between_neighbours() {
        let plan = plan(2048, 1536, 512, 32);
        let tiles = plan.tiles();
        for tile in &tiles {
            if tile.col + 1 < plan.cols() {
                let right = tiles
                    .iter()
                    .find(|other| other.row == tile.row && other.col == tile.col + 1)
                    .expect("vecino derecho");
                assert_eq!(tile.overlap_right, right.overlap_left);
            }
        }
    }

    #[test]
    fn last_tile_ends_exactly_at_the_canvas_edge() {
        let plan = plan(2048, 1536, 512, 32);
        for tile in plan.tiles() {
            if tile.col + 1 == plan.cols() {
                assert_eq!(tile.canvas_x + tile.read_w, plan.canvas_width);
            }
            if tile.row + 1 == plan.rows() {
                assert_eq!(tile.canvas_y + tile.read_h, plan.canvas_height);
            }
        }
    }

    #[test]
    fn a_tile_that_would_not_advance_is_not_duplicated() {
        // Caso limite: el canvas mide justo dos tiles. El ultimo origen coincide
        // con el borde derecho, que ya es el del tile anterior.
        let plan = plan(1024, 1024, 512, 32);
        let starts = &plan.starts_x;
        assert!(starts.windows(2).all(|pair| pair[1] > pair[0]), "{starts:?}");
    }

    #[test]
    fn absurd_overlap_does_not_hang() {
        // Un solape mayor que el tile dejaria paso cero.
        let plan = TilePlan::new(4096, 4096, 256, 4096, 32).expect("no debe colgarse");
        assert!(plan.count() > 1);
        assert!(plan.count() <= MAX_TILES as u32);
    }

    #[test]
    fn zero_sized_image_is_rejected() {
        assert!(TilePlan::new(0, 100, 512, 32, 32).is_err());
    }

    #[test]
    fn tiny_tile_is_rejected() {
        assert!(TilePlan::new(100, 100, 16, 4, 32).is_err());
    }

    #[test]
    fn output_size_matches_the_requested_scale() {
        let plan = plan(1000, 700, 512, 32);
        assert_eq!(plan.output_size(2), (2000, 1400));
        assert_eq!(plan.output_size(4), (4000, 2800));
        assert_eq!(plan.output_size(8), (8000, 5600));
    }

    #[test]
    fn window_weights_sum_to_one_in_the_overlap() {
        // El corazon del algoritmo: en una zona de solape de anchura `ov`, la
        // rampa de bajada de un tile y la de subida del siguiente suman 1.
        let len = 128u32;
        let overlap = 32u32;

        let left = axis_weights(len, 0, overlap);
        let right = axis_weights(len, overlap, 0);

        // `left` ocupa [0,len) y `right` empieza en len-overlap.
        for k in 0..overlap {
            let from_left = left[(len - overlap + k) as usize];
            let from_right = right[k as usize];
            let total = from_left + from_right;
            assert!(
                (total - 1.0).abs() < 1e-5,
                "en el offset {k} los pesos suman {total}"
            );
        }
    }

    #[test]
    fn weights_are_flat_outside_the_overlap() {
        let weights = axis_weights(128, 32, 32);
        assert!((weights[64] - 1.0).abs() < f32::EPSILON);
        assert!(weights[0] < 0.05);
        assert!(weights[127] < 0.05);
    }

    #[test]
    fn coverage_is_exactly_one_everywhere() {
        // Ningun pixel puede quedarse sin cobertura ni recibir peso de mas.
        for (width, height, tile, overlap) in [
            (1024u32, 1024u32, 512u32, 32u32),
            (2000, 1300, 512, 32),
            (300, 300, 512, 32),
            (4096, 4096, 1024, 64),
            (777, 333, 256, 16),
        ] {
            let plan = TilePlan::new(width, height, tile, overlap, 32).expect("plan");
            let scale = 4;
            let coverage = coverage(&plan, scale).expect("cobertura");

            let worst = coverage
                .iter()
                .map(|value| (value - 1.0).abs())
                .fold(0.0f32, f32::max);

            assert!(
                worst < 1e-5,
                "{width}x{height} tile={tile} ov={overlap}: desviacion maxima {worst}"
            );
        }
    }

    #[test]
    fn compositing_a_constant_image_preserves_the_value() {
        let plan = plan(2000, 1300, 512, 32);
        let scale = 4;
        let channels = 3;
        let (out_w, out_h) = plan.output_size(scale);

        let result = composite(&plan, scale, channels, |tile| {
            let (tile_w, tile_h) = tile.output_size(scale);
            Ok(vec![0.42f32; (tile_w as usize) * (tile_h as usize) * channels])
        })
        .expect("composicion");

        assert_eq!(result.len(), (out_w as usize) * (out_h as usize) * channels);
        let worst = result
            .iter()
            .map(|value| (value - 0.42).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 1e-5, "desviacion {worst}");
    }

    #[test]
    fn geometry_is_correct_and_seam_free() {
        // Cada tile devuelve el valor verdadero de la imagen sintetica. Si el
        // mapeo de coordenadas o los pesos estuvieran mal, la reconstruccion
        // fallaria o aparecerian escalones.
        let plan = plan(2000, 1300, 512, 32);
        let scale = 2;
        let channels = 1;
        let (out_w, out_h) = plan.output_size(scale);

        let truth = |x: u32, y: u32| (x as f32) * 0.001 + (y as f32) * 0.002;

        let result = composite(&plan, scale, channels, |tile| {
            let (tile_w, tile_h) = tile.output_size(scale);
            let mut data = Vec::with_capacity((tile_w as usize) * (tile_h as usize));
            for local_y in 0..tile_h {
                for local_x in 0..tile_w {
                    data.push(truth(tile.canvas_x * scale + local_x, tile.canvas_y * scale + local_y));
                }
            }
            Ok(data)
        })
        .expect("composicion");

        let mut worst = 0.0f32;
        for y in 0..out_h {
            for x in 0..out_w {
                let expected = truth(x, y);
                let actual = result[(y as usize) * (out_w as usize) + x as usize];
                worst = worst.max((actual - expected).abs());
            }
        }

        assert!(worst < 1e-3, "error maximo de reconstruccion: {worst}");
    }

    #[test]
    fn a_tile_returning_the_wrong_size_fails_loudly() {
        let plan = plan(1000, 1000, 512, 32);
        let error = composite(&plan, 4, 3, |_tile| Ok(vec![0.0f32; 10]));
        assert!(error.is_err(), "un tile con tamano incorrecto debe fallar");
    }

    #[test]
    fn zero_channels_is_rejected() {
        let plan = plan(1000, 1000, 512, 32);
        assert!(composite(&plan, 4, 0, |_tile| Ok(Vec::new())).is_err());
    }
}
