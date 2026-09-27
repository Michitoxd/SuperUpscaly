//! Motor de pipelines declarativos.
//!
//! Los pipelines viven en `pipelines.json` (embebido y sobrescribible por el
//! usuario) y describen una secuencia de etapas con condiciones opcionales.
//!
//! Decision de diseno (ADR-006): las condiciones **no** se evaluan con un motor
//! de expresiones generico ni con un lenguaje de scripting embebido. Se compilan
//! a este AST cerrado, con un conjunto fijo de operadores. Un archivo de usuario
//! con sintaxis invalida se rechaza con un error claro; nunca se ejecuta codigo
//! arbitrario.
//!
//! Sintaxis soportada:
//!
//! ```jsonc
//! true
//! { "and": [ <cond>, <cond> ] }
//! { "or":  [ <cond>, <cond> ] }
//! { "not": <cond> }
//! { "path": "analysis.noise", "op": "gt", "value": 0.35 }
//! { "path": "prefs.sharpen",  "truthy": true }
//! ```

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{json, Value};

use crate::analysis::FaceBox;
use crate::error::{SuError, SuResult};
use crate::types::{
    ContentKind, DenoiseChoice, FaceRestoreChoice, Mode, ModelChainMode, Scale, StageId,
};

// ---------------------------------------------------------------------------
// Valores y contexto de evaluacion
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    Num(f64),
    Bool(bool),
    Str(String),
}

/// Variables visibles desde una condicion. Es un tipo concreto, no un mapa
/// abierto: una condicion que referencia una variable inexistente es un error,
/// no un `false` silencioso.
#[derive(Debug, Clone)]
pub struct EvalVars {
    // --- analysis.* ---
    pub noise: f32,
    pub blockiness: f32,
    pub face_count: usize,
    /// Cajas de los rostros detectados, en coordenadas normalizadas `0..1`.
    ///
    /// No basta con `face_count`: la etapa que declara `onlyOnFaces` tiene que
    /// saber **donde** estan las caras para recortarlas. Con solo el numero, esa
    /// etapa pasaba la imagen entera por el modelo, que es lo contrario de lo
    /// que anuncia su nombre.
    pub faces: Vec<FaceBox>,
    pub kind: ContentKind,
    pub kind_confidence: f32,
    pub megapixels: f32,
    pub estimated_output_mp: f32,
    pub has_alpha: bool,
    // --- prefs.* ---
    pub mode: Mode,
    pub scale: Scale,
    pub face_restore: FaceRestoreChoice,
    pub denoise: DenoiseChoice,
    pub sharpen: bool,
    pub model_chain_mode: ModelChainMode,
    pub max_output_mp: f32,
    // --- hardware.* ---
    pub free_vram_mb: u64,
    pub is_cpu: bool,
    pub provider: String,
    pub cores: usize,
}

impl Default for EvalVars {
    fn default() -> Self {
        Self {
            noise: 0.0,
            blockiness: 0.0,
            face_count: 0,
            faces: Vec::new(),
            kind: ContentKind::Unknown,
            kind_confidence: 0.0,
            megapixels: 0.0,
            estimated_output_mp: 0.0,
            has_alpha: false,
            mode: Mode::Photo,
            scale: Scale::X4,
            face_restore: FaceRestoreChoice::Auto,
            denoise: DenoiseChoice::Auto,
            sharpen: false,
            model_chain_mode: ModelChainMode::Auto,
            max_output_mp: 800.0,
            free_vram_mb: 0,
            is_cpu: true,
            provider: "CPU".to_string(),
            cores: 1,
        }
    }
}

impl EvalVars {
    /// Resuelve una ruta `grupo.campo`. Devuelve `None` si no existe, para que
    /// el evaluador pueda distinguir "variable desconocida" de "valor falso".
    pub fn lookup(&self, path: &str) -> Option<Scalar> {
        let (group, field) = path.split_once('.')?;
        let value = match (group, field) {
            ("analysis", "noise") => Scalar::Num(self.noise as f64),
            ("analysis", "blockiness") => Scalar::Num(self.blockiness as f64),
            ("analysis", "faces") => Scalar::Num(self.face_count as f64),
            ("analysis", "kind") => Scalar::Str(content_kind_str(self.kind).to_string()),
            ("analysis", "confidence") => Scalar::Num(self.kind_confidence as f64),
            ("analysis", "megapixels") => Scalar::Num(self.megapixels as f64),
            ("analysis", "estimatedOutputMp") => Scalar::Num(self.estimated_output_mp as f64),
            ("analysis", "hasAlpha") => Scalar::Bool(self.has_alpha),
            ("prefs", "mode") => Scalar::Str(self.mode.as_str().to_string()),
            ("prefs", "scale") => Scalar::Num(self.scale.factor() as f64),
            ("prefs", "faceRestore") => Scalar::Str(face_restore_str(self.face_restore).to_string()),
            ("prefs", "denoise") => Scalar::Str(denoise_str(self.denoise).to_string()),
            ("prefs", "sharpen") => Scalar::Bool(self.sharpen),
            ("prefs", "modelChainMode") => Scalar::Str(model_chain_str(self.model_chain_mode).to_string()),
            ("prefs", "maxOutputMp") => Scalar::Num(self.max_output_mp as f64),
            ("hardware", "freeVramMb") => Scalar::Num(self.free_vram_mb as f64),
            ("hardware", "isCpu") => Scalar::Bool(self.is_cpu),
            ("hardware", "provider") => Scalar::Str(self.provider.clone()),
            ("hardware", "cores") => Scalar::Num(self.cores as f64),
            _ => return None,
        };
        Some(value)
    }

    /// Lista de rutas validas. Se usa para dar un mensaje de error util cuando
    /// alguien escribe mal una variable en su `pipelines.user.json`.
    pub const KNOWN_PATHS: &'static [&'static str] = &[
        "analysis.noise",
        "analysis.blockiness",
        "analysis.faces",
        "analysis.kind",
        "analysis.confidence",
        "analysis.megapixels",
        "analysis.estimatedOutputMp",
        "analysis.hasAlpha",
        "prefs.mode",
        "prefs.scale",
        "prefs.faceRestore",
        "prefs.denoise",
        "prefs.sharpen",
        "prefs.modelChainMode",
        "prefs.maxOutputMp",
        "hardware.freeVramMb",
        "hardware.isCpu",
        "hardware.provider",
        "hardware.cores",
    ];
}

fn content_kind_str(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Photo => "photo",
        ContentKind::Illustration => "illustration",
        ContentKind::Unknown => "unknown",
    }
}

fn face_restore_str(choice: FaceRestoreChoice) -> &'static str {
    match choice {
        FaceRestoreChoice::Off => "off",
        FaceRestoreChoice::Auto => "auto",
        FaceRestoreChoice::Low => "low",
        FaceRestoreChoice::Medium => "medium",
        FaceRestoreChoice::High => "high",
    }
}

fn denoise_str(choice: DenoiseChoice) -> &'static str {
    match choice {
        DenoiseChoice::Off => "off",
        DenoiseChoice::Auto => "auto",
        DenoiseChoice::On => "on",
    }
}

fn model_chain_str(mode: ModelChainMode) -> &'static str {
    match mode {
        ModelChainMode::Auto => "auto",
        ModelChainMode::Manual => "manual",
    }
}

// ---------------------------------------------------------------------------
// Condiciones
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CmpOp {
    fn parse(text: &str) -> SuResult<Self> {
        match text {
            "eq" => Ok(Self::Eq),
            "ne" => Ok(Self::Ne),
            "lt" => Ok(Self::Lt),
            "le" => Ok(Self::Le),
            "gt" => Ok(Self::Gt),
            "ge" => Ok(Self::Ge),
            other => Err(SuError::Internal(format!(
                "operador de comparacion desconocido: '{other}' (validos: eq, ne, lt, le, gt, ge)"
            ))),
        }
    }
}

impl fmt::Display for CmpOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Eq => "eq",
            Self::Ne => "ne",
            Self::Lt => "lt",
            Self::Le => "le",
            Self::Gt => "gt",
            Self::Ge => "ge",
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    Literal(bool),
    And(Vec<Condition>),
    Or(Vec<Condition>),
    Not(Box<Condition>),
    Compare {
        path: String,
        op: CmpOp,
        value: Scalar,
    },
    Truthy {
        path: String,
        negated: bool,
    },
}

impl Condition {
    /// Compila un `serde_json::Value` al AST. Rechaza cualquier forma que no
    /// reconozca en lugar de ignorarla.
    pub fn parse(value: &Value) -> SuResult<Self> {
        match value {
            Value::Bool(flag) => Ok(Self::Literal(*flag)),
            Value::Object(map) => {
                if let Some(inner) = map.get("and") {
                    return Ok(Self::And(Self::parse_list(inner, "and")?));
                }
                if let Some(inner) = map.get("or") {
                    return Ok(Self::Or(Self::parse_list(inner, "or")?));
                }
                if let Some(inner) = map.get("not") {
                    return Ok(Self::Not(Box::new(Self::parse(inner)?)));
                }

                let path = map
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        SuError::Internal(
                            "condicion no valida: falta 'path' (o 'and'/'or'/'not')".to_string(),
                        )
                    })?
                    .to_string();

                if !EvalVars::KNOWN_PATHS.contains(&path.as_str()) {
                    return Err(SuError::Internal(format!(
                        "variable desconocida '{path}' en una condicion del pipeline"
                    )));
                }

                if let Some(op_text) = map.get("op").and_then(Value::as_str) {
                    let raw_value = map.get("value").ok_or_else(|| {
                        SuError::Internal(format!("la condicion sobre '{path}' usa 'op' sin 'value'"))
                    })?;
                    return Ok(Self::Compare {
                        path,
                        op: CmpOp::parse(op_text)?,
                        value: scalar_from_json(raw_value)?,
                    });
                }

                if let Some(truthy) = map.get("truthy").and_then(Value::as_bool) {
                    return Ok(Self::Truthy {
                        path,
                        negated: !truthy,
                    });
                }

                Err(SuError::Internal(format!(
                    "la condicion sobre '{path}' necesita 'op' + 'value', o 'truthy'"
                )))
            }
            other => Err(SuError::Internal(format!(
                "condicion no valida: se esperaba un objeto o un booleano, no {other}"
            ))),
        }
    }

    fn parse_list(value: &Value, keyword: &str) -> SuResult<Vec<Condition>> {
        let array = value.as_array().ok_or_else(|| {
            SuError::Internal(format!("'{keyword}' espera una lista de condiciones"))
        })?;
        if array.is_empty() {
            return Err(SuError::Internal(format!(
                "'{keyword}' necesita al menos una condicion"
            )));
        }
        array.iter().map(Self::parse).collect()
    }

    pub fn evaluate(&self, vars: &EvalVars) -> SuResult<bool> {
        match self {
            Self::Literal(flag) => Ok(*flag),
            Self::And(list) => {
                for condition in list {
                    if !condition.evaluate(vars)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Self::Or(list) => {
                for condition in list {
                    if condition.evaluate(vars)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Self::Not(inner) => Ok(!inner.evaluate(vars)?),
            Self::Truthy { path, negated } => {
                let value = vars.lookup(path).ok_or_else(|| unknown_variable(path))?;
                let truthy = match value {
                    Scalar::Bool(flag) => flag,
                    Scalar::Num(number) => number != 0.0,
                    // Una cadena de opcion se considera activa si no es "off".
                    Scalar::Str(text) => !text.is_empty() && text != "off",
                };
                Ok(if *negated { !truthy } else { truthy })
            }
            Self::Compare { path, op, value } => {
                let actual = vars.lookup(path).ok_or_else(|| unknown_variable(path))?;
                compare(&actual, *op, value)
            }
        }
    }
}

fn unknown_variable(path: &str) -> SuError {
    SuError::Internal(format!("variable desconocida '{path}' al evaluar el pipeline"))
}

fn scalar_from_json(value: &Value) -> SuResult<Scalar> {
    match value {
        Value::Number(number) => number
            .as_f64()
            .map(Scalar::Num)
            .ok_or_else(|| SuError::Internal(format!("numero no representable: {number}"))),
        Value::Bool(flag) => Ok(Scalar::Bool(*flag)),
        Value::String(text) => Ok(Scalar::Str(text.clone())),
        other => Err(SuError::Internal(format!(
            "valor no comparable en una condicion: {other}"
        ))),
    }
}

fn compare(actual: &Scalar, op: CmpOp, expected: &Scalar) -> SuResult<bool> {
    match (actual, expected) {
        (Scalar::Num(a), Scalar::Num(b)) => Ok(match op {
            CmpOp::Eq => a == b,
            CmpOp::Ne => a != b,
            CmpOp::Lt => a < b,
            CmpOp::Le => a <= b,
            CmpOp::Gt => a > b,
            CmpOp::Ge => a >= b,
        }),
        (Scalar::Bool(a), Scalar::Bool(b)) => match op {
            CmpOp::Eq => Ok(a == b),
            CmpOp::Ne => Ok(a != b),
            _ => Err(SuError::Internal(format!(
                "el operador '{op}' no se aplica a booleanos"
            ))),
        },
        (Scalar::Str(a), Scalar::Str(b)) => match op {
            CmpOp::Eq => Ok(a == b),
            CmpOp::Ne => Ok(a != b),
            _ => Err(SuError::Internal(format!(
                "el operador '{op}' no se aplica a cadenas"
            ))),
        },
        (a, b) => Err(SuError::Internal(format!(
            "comparacion entre tipos distintos: {a:?} {op} {b:?}"
        ))),
    }
}

impl Serialize for Condition {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(serializer)
    }
}

impl Condition {
    /// Reconstruye la forma JSON original. Se usa para que `GET /v1/pipelines`
    /// devuelva exactamente lo que el sidecar tiene cargado.
    pub fn to_json(&self) -> Value {
        match self {
            Self::Literal(flag) => json!(flag),
            Self::And(list) => json!({ "and": list.iter().map(Self::to_json).collect::<Vec<_>>() }),
            Self::Or(list) => json!({ "or": list.iter().map(Self::to_json).collect::<Vec<_>>() }),
            Self::Not(inner) => json!({ "not": inner.to_json() }),
            Self::Compare { path, op, value } => json!({
                "path": path,
                "op": op.to_string(),
                "value": scalar_to_json(value),
            }),
            Self::Truthy { path, negated } => json!({ "path": path, "truthy": !negated }),
        }
    }
}

fn scalar_to_json(value: &Scalar) -> Value {
    match value {
        Scalar::Num(number) => json!(number),
        Scalar::Bool(flag) => json!(flag),
        Scalar::Str(text) => json!(text),
    }
}

impl<'de> Deserialize<'de> for Condition {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Condition::parse(&value).map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Etapas y pipelines
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StageOp {
    Analyze,
    Model,
    Resize,
    Unsharp,
    #[serde(rename = "denoiseClassic")]
    DenoiseClassic,
    Compose,
}

impl StageOp {
    /// Etapa del pipeline a la que corresponde esta operacion, para poder
    /// reportar progreso con el vocabulario que entiende la interfaz.
    pub const fn stage_id(self) -> StageId {
        match self {
            Self::Analyze => StageId::Analyze,
            Self::Model => StageId::Upscale,
            Self::Resize => StageId::Upscale,
            Self::Unsharp => StageId::Sharpen,
            Self::DenoiseClassic => StageId::Denoise,
            Self::Compose => StageId::Face,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stage {
    pub id: String,
    pub op: StageOp,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_model: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<Condition>,

    /// `"auto"` o un tamano explicito en pixeles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tiling: Option<Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_out: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend: Option<f32>,
    /// Peso del `blend` calculado a partir de lo que mide el analisis.
    ///
    /// Existe porque un peso fijo desaprovecha la informacion que ya se tiene: la
    /// reduccion de ruido se declara con un unico `blend: 0.9` tanto para una foto
    /// con ruido moderado —donde conviene un 0.5, para no lavar el detalle— como
    /// para una muy degradada, donde conviene el 1.0. Con esto la etapa declara los
    /// dos extremos y el analisis decide donde cae.
    ///
    /// Si una etapa declara las dos cosas, manda esto: un peso que depende del
    /// analisis es una decision mas concreta que un numero fijo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend_from: Option<BlendFrom>,
    #[serde(default)]
    pub only_on_faces: bool,
    /// Multiplica el solape. La segunda pasada de 8x usa 2 para no acumular costuras.
    #[serde(default)]
    pub overlap_boost: u32,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kernel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factor: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f32>,
}

/// Peso de una etapa expresado como interpolacion de una variable del analisis.
///
/// `min` es el peso cuando la variable vale 0 y `max` cuando vale 1; en medio se
/// interpola linealmente. La variable se recorta a `0..1` antes de interpolar, y
/// una ruta que no exista es un **error**, no un cero silencioso: una errata en
/// `"blendFrom": { "path": "analysis.noisse" }` dejaria la etapa sin efecto sin que
/// nada lo dijera, que es justo lo que este proyecto no hace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlendFrom {
    pub path: String,
    pub min: f32,
    pub max: f32,
}

impl BlendFrom {
    /// Peso que corresponde a estas variables.
    pub fn weight(&self, vars: &EvalVars) -> SuResult<f32> {
        let value = match vars.lookup(&self.path) {
            Some(Scalar::Num(value)) => value as f32,
            Some(other) => {
                return Err(SuError::Internal(format!(
                    "'{}' vale {other:?} y un peso necesita un numero",
                    self.path
                )))
            }
            None => return Err(unknown_variable(&self.path)),
        };

        let position = value.clamp(0.0, 1.0);
        Ok(self.min + (self.max - self.min) * position)
    }
}

impl Stage {
    /// `true` si la etapa debe ejecutarse con estas variables. Sin condicion,
    /// siempre se ejecuta.
    pub fn is_active(&self, vars: &EvalVars) -> SuResult<bool> {
        match &self.when {
            None => Ok(true),
            Some(condition) => condition.evaluate(vars),
        }
    }

    /// Peso efectivo del `blend`: el fijo, o el que resuelve el analisis.
    ///
    /// Se calcula una sola vez por etapa y por imagen, y no en cada punto de uso:
    /// el peso de la restauracion facial tambien sale de aqui, y dos caminos que
    /// resolvieran lo mismo por su cuenta acabarian discrepando.
    pub fn blend_weight(&self, vars: &EvalVars) -> SuResult<Option<f32>> {
        match &self.blend_from {
            Some(source) => Ok(Some(source.weight(vars)?)),
            None => Ok(self.blend),
        }
    }

    /// Etapa que se reporta a la interfaz mientras corre esta operacion.
    pub fn reported_stage(&self) -> StageId {
        match (self.op, self.id.as_str()) {
            (StageOp::Model, "denoise") => StageId::Denoise,
            (StageOp::Model, "face") => StageId::Face,
            (StageOp::Model, "lineclean") => StageId::Denoise,
            (op, _) => op.stage_id(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pipeline {
    pub id: String,
    pub mode: Mode,
    pub scale: Scale,
    #[serde(default)]
    pub description: String,
    pub stages: Vec<Stage>,
}

impl Pipeline {
    /// Etapas que realmente se ejecutarian con estas variables. Es lo que la
    /// interfaz muestra en el resumen: "¿por que esta imagen salio distinta?"
    /// tiene una respuesta visible.
    pub fn effective_stages(&self, vars: &EvalVars) -> SuResult<Vec<StageId>> {
        let mut executed = Vec::new();
        for stage in &self.stages {
            if stage.is_active(vars)? {
                executed.push(stage.reported_stage());
            }
        }
        Ok(executed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineSet {
    pub version: u32,
    pub pipelines: Vec<Pipeline>,
}

impl PipelineSet {
    /// Pipelines embebidos. Es la referencia que se usa si no hay ningun
    /// `pipelines.user.json` valido.
    pub fn embedded() -> SuResult<Self> {
        let raw = include_str!("../pipelines.default.json");
        serde_json::from_str(raw)
            .map_err(|error| SuError::Internal(format!("pipelines embebidos invalidos: {error}")))
    }

    /// Carga un conjunto desde texto. Si falla, el llamador debe caer a
    /// `embedded()` y avisar; nunca dejar la app sin pipelines.
    pub fn from_json(raw: &str) -> SuResult<Self> {
        serde_json::from_str(raw)
            .map_err(|error| SuError::Internal(format!("pipelines no validos: {error}")))
    }

    pub fn get(&self, mode: Mode, scale: Scale) -> Option<&Pipeline> {
        self.pipelines
            .iter()
            .find(|pipeline| pipeline.mode == mode && pipeline.scale == scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> EvalVars {
        EvalVars {
            noise: 0.50,
            blockiness: 0.10,
            face_count: 2,
            faces: vec![FaceBox {
                x: 0.40,
                y: 0.30,
                w: 0.10,
                h: 0.12,
                confidence: 0.9,
            }],
            kind: ContentKind::Photo,
            kind_confidence: 0.9,
            megapixels: 2.0,
            estimated_output_mp: 32.0,
            mode: Mode::Photo,
            scale: Scale::X4,
            face_restore: FaceRestoreChoice::Auto,
            denoise: DenoiseChoice::Auto,
            sharpen: true,
            model_chain_mode: ModelChainMode::Auto,
            free_vram_mb: 8192,
            is_cpu: false,
            provider: "TensorRT".to_string(),
            cores: 16,
            ..EvalVars::default()
        }
    }

    fn parse(json: Value) -> Condition {
        Condition::parse(&json).expect("la condicion deberia ser valida")
    }

    /// Etapa minima con un `blendFrom`, para las pruebas del peso variable.
    fn weighted_stage(path: &str, min: f32, max: f32) -> Stage {
        serde_json::from_value(json!({
            "id": "denoise",
            "op": "model",
            "model": "scunet-color",
            "blend": 0.9,
            "blendFrom": { "path": path, "min": min, "max": max }
        }))
        .expect("etapa con blendFrom")
    }

    #[test]
    fn the_blend_weight_interpolates_between_its_endpoints() {
        let stage = weighted_stage("analysis.noise", 0.5, 1.0);

        let mut quiet = vars();
        quiet.noise = 0.0;
        assert_eq!(stage.blend_weight(&quiet).unwrap(), Some(0.5));

        let mut loud = vars();
        loud.noise = 1.0;
        assert_eq!(stage.blend_weight(&loud).unwrap(), Some(1.0));

        let mut middle = vars();
        middle.noise = 0.5;
        assert_eq!(stage.blend_weight(&middle).unwrap(), Some(0.75));

        // Y manda sobre el `blend` fijo que declare la etapa: los dos estan puestos
        // en `weighted_stage` a proposito.
        assert_ne!(stage.blend_weight(&middle).unwrap(), Some(0.9));
    }

    #[test]
    fn the_blend_variable_is_clamped_and_a_typo_is_an_error() {
        let stage = weighted_stage("analysis.noise", 0.5, 1.0);

        // Una variable fuera de rango no se extrapola: se recorta.
        let mut beyond = vars();
        beyond.noise = 4.0;
        assert_eq!(stage.blend_weight(&beyond).unwrap(), Some(1.0));

        // Una ruta mal escrita no vale 0.5 ni 0.9: es un error, porque callarlo
        // dejaria la etapa con un peso que nadie eligio.
        for path in ["analysis.noisse", "analysis.kind", "prefs.sharpen"] {
            let stage = weighted_stage(path, 0.5, 1.0);
            assert!(
                stage.blend_weight(&vars()).is_err(),
                "'{path}' deberia ser un error"
            );
        }
    }

    #[test]
    fn the_embedded_denoise_declares_a_weight_that_depends_on_the_noise() {
        // Propiedad del catalogo, no del codigo: la etapa de reduccion de ruido de
        // los tres pipelines de foto tiene que dejar de tener un peso fijo. Si
        // alguien vuelve a poner un `blend: 0.9`, esta prueba lo dice.
        let set = PipelineSet::embedded().expect("pipelines embebidos");

        for scale in [Scale::X2, Scale::X4, Scale::X8] {
            let pipeline = set.get(Mode::Photo, scale).expect("pipeline de foto");
            let denoise = pipeline
                .stages
                .iter()
                .find(|stage| stage.id == "denoise")
                .expect("etapa de denoise");

            let mut quiet = vars();
            quiet.noise = 0.4;
            let mut loud = vars();
            loud.noise = 1.0;

            let quiet_weight = denoise.blend_weight(&quiet).unwrap().expect("peso");
            let loud_weight = denoise.blend_weight(&loud).unwrap().expect("peso");
            assert!(
                loud_weight > quiet_weight,
                "x{}: el peso ({quiet_weight}) no crece con el ruido ({loud_weight})",
                scale.factor()
            );
        }
    }

    #[test]
    fn literal_booleans_evaluate() {
        assert!(parse(json!(true)).evaluate(&vars()).unwrap());
        assert!(!parse(json!(false)).evaluate(&vars()).unwrap());
    }

    #[test]
    fn numeric_comparison_works() {
        let condition = parse(json!({ "path": "analysis.noise", "op": "gt", "value": 0.35 }));
        assert!(condition.evaluate(&vars()).unwrap());

        let below = parse(json!({ "path": "analysis.noise", "op": "lt", "value": 0.35 }));
        assert!(!below.evaluate(&vars()).unwrap());
    }

    #[test]
    fn string_comparison_works() {
        let condition = parse(json!({ "path": "prefs.faceRestore", "op": "ne", "value": "off" }));
        assert!(condition.evaluate(&vars()).unwrap());
    }

    #[test]
    fn truthy_treats_off_as_false() {
        let mut v = vars();
        v.denoise = DenoiseChoice::Off;
        let condition = parse(json!({ "path": "prefs.denoise", "truthy": true }));
        assert!(!condition.evaluate(&v).unwrap());

        v.denoise = DenoiseChoice::On;
        assert!(condition.evaluate(&v).unwrap());
    }

    #[test]
    fn boolean_comparison_works() {
        let condition = parse(json!({ "path": "hardware.isCpu", "op": "eq", "value": false }));
        assert!(condition.evaluate(&vars()).unwrap());
    }

    #[test]
    fn and_or_not_compose() {
        let condition = parse(json!({
            "and": [
                { "path": "analysis.noise", "op": "gt", "value": 0.35 },
                { "not": { "path": "prefs.sharpen", "op": "eq", "value": false } }
            ]
        }));
        assert!(condition.evaluate(&vars()).unwrap());

        let either = parse(json!({
            "or": [
                { "path": "analysis.noise", "op": "gt", "value": 0.99 },
                { "path": "analysis.faces", "op": "gt", "value": 0 }
            ]
        }));
        assert!(either.evaluate(&vars()).unwrap());
    }

    #[test]
    fn unknown_variable_is_rejected_at_parse_time() {
        let error = Condition::parse(&json!({
            "path": "analysis.noisee",
            "op": "gt",
            "value": 0.35
        }));
        assert!(error.is_err(), "una variable mal escrita debe fallar");
    }

    #[test]
    fn unknown_operator_is_rejected() {
        let error = Condition::parse(&json!({
            "path": "analysis.noise",
            "op": "aproximately",
            "value": 0.35
        }));
        assert!(error.is_err());
    }

    #[test]
    fn comparing_different_types_is_an_error_not_a_silent_false() {
        let condition = parse(json!({ "path": "analysis.noise", "op": "gt", "value": "mucho" }));
        assert!(condition.evaluate(&vars()).is_err());
    }

    #[test]
    fn ordering_operators_are_rejected_on_strings() {
        let condition = parse(json!({ "path": "analysis.kind", "op": "gt", "value": "photo" }));
        assert!(condition.evaluate(&vars()).is_err());
    }

    #[test]
    fn condition_round_trips_through_json() {
        let original = parse(json!({
            "and": [
                { "path": "analysis.noise", "op": "gt", "value": 0.35 },
                { "path": "prefs.sharpen", "truthy": true }
            ]
        }));
        let restored = Condition::parse(&original.to_json()).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn empty_and_list_is_rejected() {
        assert!(Condition::parse(&json!({ "and": [] })).is_err());
    }

    #[test]
    fn embedded_pipelines_load_and_resolve() {
        let set = PipelineSet::embedded().expect("los pipelines embebidos deben cargar");

        let photo = set
            .get(Mode::Photo, Scale::X4)
            .expect("falta el pipeline photo:4x");
        assert!(photo.stages.len() >= 4);

        let illustration = set
            .get(Mode::Illustration, Scale::X4)
            .expect("falta el pipeline illustration:4x");
        // En Dibujo/Anime no hay restauracion facial.
        assert!(!illustration.stages.iter().any(|stage| stage.id == "face"));
    }

    #[test]
    fn effective_stages_skip_disabled_conditions() {
        let set = PipelineSet::embedded().unwrap();
        let photo = set.get(Mode::Photo, Scale::X4).unwrap();

        let mut v = vars();
        v.noise = 0.05; // limpia: no se activa el denoise
        v.face_count = 0; // sin rostros: no se activa la restauracion facial
        v.sharpen = false;

        let stages = photo.effective_stages(&v).unwrap();
        assert!(!stages.contains(&StageId::Denoise));
        assert!(!stages.contains(&StageId::Face));
        assert!(stages.contains(&StageId::Upscale));

        let mut noisy = vars();
        noisy.noise = 0.80;
        noisy.face_count = 3;
        let stages = photo.effective_stages(&noisy).unwrap();
        assert!(stages.contains(&StageId::Denoise));
        assert!(stages.contains(&StageId::Face));
    }

    #[test]
    fn every_embedded_condition_is_evaluable() {
        // Red de seguridad: recorre todas las condiciones embebidas con unas
        // variables realistas y exige que ninguna falle.
        let set = PipelineSet::embedded().unwrap();
        for pipeline in &set.pipelines {
            pipeline
                .effective_stages(&vars())
                .unwrap_or_else(|error| panic!("{} fallo al evaluar: {error}", pipeline.id));
        }
    }

    #[test]
    fn every_pipeline_lands_on_the_scale_it_promises() {
        // Este test existe por un motivo concreto: el error mas facil de cometer
        // en un pipeline de upscaling es que el producto de los factores no sea
        // la escala que anuncia el identificador. Los pipelines de 8x llegaron a
        // hacer x4 + x4 = 16x por faltar la reduccion intermedia.
        let set = PipelineSet::embedded().unwrap();

        for pipeline in &set.pipelines {
            let mut factor = 1.0f64;

            for stage in &pipeline.stages {
                match stage.op {
                    // `unwrap_or(1)` y no `unwrap_or(escala del modelo)`: `su-core` no
                    // conoce los modelos, solo los pipelines. Una etapa que no declara
                    // `scaleOut` se cuenta como que no cambia la escala, y quien
                    // garantiza que eso es cierto es el test de `su-cli`
                    // `a_model_stage_without_a_scale_must_use_a_model_that_does_not_scale`,
                    // que cruza los pipelines con el manifiesto. Si esa invariante se
                    // rompe, el runner lo caza en ejecucion con `ScaleNotReached`.
                    StageOp::Model => factor *= stage.scale_out.unwrap_or(1) as f64,
                    StageOp::Resize => factor *= stage.factor.unwrap_or(1.0) as f64,
                    StageOp::Analyze | StageOp::Unsharp | StageOp::DenoiseClassic | StageOp::Compose => {}
                }
            }

            let declared = pipeline.scale.factor() as f64;
            assert!(
                (factor - declared).abs() < 1e-6,
                "el pipeline '{}' declara x{declared} pero sus etapas dan x{factor}",
                pipeline.id
            );
        }
    }

    #[test]
    fn every_pipeline_starts_by_analysing() {
        // El analisis alimenta las condiciones de todas las demas etapas: si
        // faltara, las variables quedarian a cero y se omitirian etapas sin motivo.
        let set = PipelineSet::embedded().unwrap();
        for pipeline in &set.pipelines {
            let first = pipeline.stages.first().expect("un pipeline vacio no sirve");
            assert_eq!(
                first.op,
                StageOp::Analyze,
                "el pipeline '{}' no empieza analizando",
                pipeline.id
            );
        }
    }

    #[test]
    fn eight_x_pipelines_reduce_between_the_two_model_passes() {
        // Con modelos x4, dos pasadas dan 16x. Para aterrizar en 8x hace falta
        // exactamente una reduccion intermedia; sin ella, el resultado tendria el
        // doble de lado del prometido.
        let set = PipelineSet::embedded().unwrap();

        for pipeline in set.pipelines.iter().filter(|p| p.scale == Scale::X8) {
            let halves = pipeline
                .stages
                .iter()
                .filter(|stage| stage.op == StageOp::Resize && stage.factor.unwrap_or(1.0) < 1.0)
                .count();

            assert_eq!(
                halves, 1,
                "el pipeline '{}' necesita exactamente una reduccion intermedia, tiene {halves}",
                pipeline.id
            );
        }
    }
}
