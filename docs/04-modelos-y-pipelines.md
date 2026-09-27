# 04 · Modelos y pipelines

> Catálogo, cadenas de procesado, manifiesto, estrategia de escalas y guía para añadir modelos sin tocar código.

---

## 1. Formato y procedencia

Todos los modelos son **ONNX** (opset ≥ 13, preferiblemente 17) con:

- Ejes de lote y canal dinámicos (`N`, `C`) cuando el EP lo permite.
- Formato de tensor de entrada: `float32`, rango `[0,1]`, layout `NCHW`.
- Salida con **el mismo layout y rango** (nada de `uint8` ni de rangos `[0,255]`: obligar a un rango concreto es una fuente de errores de conversión).
- Nombre de archivo con hash: `models/<id>-<sha256[0:8]>.onnx`.

> El proyecto **no redistribuye** los modelos en el repositorio ni en el instalador. El manifiesto apunta a las fuentes originales y el gestor los descarga en el primer uso, mostrando la licencia.
>
> «En el primer uso» es literal: al pulsar **Upscaly**, la aplicación pregunta al
> sidecar qué modelo necesita el pipeline del modo y la escala elegidos
> (`GET /v1/pipelines`) y, si falta, lo descarga antes de aceptar el trabajo
> (`apps/desktop/src/main/models/ensure.ts`, ADR-026). Solo se bajan los modelos de
> las etapas que **escalan**: los de restauración son opcionales y se omiten con su
> motivo cuando faltan.

### Conversión de modelos existentes a ONNX

| Origen | Procedimiento |
|---|---|
| PyTorch (`.pth`, arquitecturas BasicSR/Real-ESRGAN) | `torch.onnx.export` con `dynamic_axes` en H/W, verificación numérica contra la salida de PyTorch (error máximo < 1e-4 en fp32) |
| NCNN (`.param` + `.bin`) | `pnnx` → ONNX. Verificación obligatoria por PSNR: si el PSNR de la versión ONNX baja más de 0.1 dB respecto al NCNN original, se descarta |
| TensorFlow / Keras | `tf2onnx` |
| Modelos con opcodes no soportados | Reescritura del grafo o descarte. **Nunca** se marca un modelo como válido sin haber ejecutado el test de conformidad (§6) |

---

## 2. Manifiesto (`models/manifest.json`)

```jsonc
{
  "manifestVersion": 2,
  "updatedAt": "2026-09-15T00:00:00Z",
  "models": [
    {
      "id": "4x-ultrasharp",
      "name": "4x UltraSharp",
      "kind": "photo",
      "scale": 4,
      "arch": "rrdbnet",
      "sha256": "7295b39b71f1d5882fec1ae02f55227f7ca6516f92eae6920ab2a28a39cade73",
      "sizeBytes": 33605809,
      "urls": ["https://huggingface.co/Kim2091/UltraSharp/resolve/main/ONNX/4x-UltraSharp-fp16-opset17.onnx"],
      "license": { "name": "CC-BY-NC-SA-4.0", "commercialUse": false,
                   "note": "No permite uso comercial" },
      "precision": { "fp16Safe": true, "preferred": "fp16" },
      "input": { "multipleOf": 1, "channels": 3, "minSide": 32 },
      "tiling": {
        "candidates": [1024, 768, 512, 384, 256, 192],
        "overlapDivisor": 16,
        "padTo": 32,
        "vramPerMegapixel": 420.0,
        "msPerMegapixelCpu": 9800.0
      },
      "tags": ["general", "photo", "sharp"],
      "notes": "Excelente para fotografía con detalle fino. Puede sobreenfocar imágenes ya nítidas."
    },
    {
      "id": "realesrgan-x4plus",
      "name": "Real-ESRGAN x4plus",
      "kind": "photo",
      "scale": 4,
      "arch": "rrdbnet",
      "sha256": "4fa0…",
      "license": { "name": "BSD-3-Clause", "commercialUse": true },
      "tiling": { "candidates": [1024, 768, 512, 384, 256], "overlapDivisor": 16, "padTo": 32, "vramPerMegapixel": 430.0 },
      "tags": ["general", "photo", "balanced"]
    },
    {
      "id": "realesrgan-x4plus-anime-6b",
      "name": "RealESRGAN x4plus Anime 6B",
      "kind": "illustration",
      "scale": 4,
      "arch": "rrdbnet-6b",
      "sha256": "…",
      "license": { "name": "BSD-3-Clause", "commercialUse": true },
      "tiling": { "candidates": [1024, 768, 512, 384, 256], "overlapDivisor": 16, "padTo": 32, "vramPerMegapixel": 250.0 },
      "tags": ["anime", "lineart", "balanced"]
    },
    {
      "id": "2x-animesharpv3",
      "name": "2x AnimeSharp V3",
      "kind": "illustration",
      "scale": 2,
      "sha256": "…",
      "license": { "name": "CC-BY-NC-SA-4.0", "commercialUse": false },
      "tags": ["anime", "fast", "2x"]
    },
    {
      "id": "scunet-color",
      "name": "SCUNet Color (denoise)",
      "kind": "denoise",
      "scale": 1,
      "sha256": "…",
      "license": { "name": "Apache-2.0", "commercialUse": true },
      "tags": ["denoise", "pre-process"]
    },
    {
      "id": "gfpgan-v1.4",
      "name": "GFPGAN v1.4",
      "kind": "face",
      "scale": 1,
      "sha256": "…",
      "license": { "name": "Apache-2.0", "commercialUse": true },
      "tags": ["face-restore"]
    },
    {
      "id": "codeformer",
      "name": "CodeFormer",
      "kind": "face",
      "scale": 1,
      "sha256": "…",
      "license": { "name": "S-Lab License 1.0", "commercialUse": false, "note": "Restricciones para uso comercial" },
      "tags": ["face-restore", "high-quality"]
    },
    {
      "id": "yunet-2023",
      "name": "YuNet face detector",
      "kind": "detector",
      "scale": 1,
      "sha256": "…",
      "license": { "name": "Apache-2.0", "commercialUse": true },
      "tags": ["analysis"]
    },
    {
      "id": "content-classifier-v1",
      "name": "Photo/Illustration classifier",
      "kind": "classifier",
      "scale": 1,
      "sha256": "…",
      "license": { "name": "MIT", "commercialUse": true },
      "tags": ["analysis"]
    }
  ],
  "accelerationPacks": [
    {
      "id": "nvidia-cuda-12",
      "platform": ["win32-x64", "linux-x64"],
      "provider": "CUDA",
      "sha256": "…",
      "sizeBytes": 298000000,
      "requires": { "driverMin": "525.60", "gpuVendor": "NVIDIA", "computeCapabilityMin": 7.5 }
    },
    {
      "id": "nvidia-tensorrt-10",
      "platform": ["win32-x64", "linux-x64"],
      "provider": "TensorRT",
      "sha256": "…",
      "sizeBytes": 412000000,
      "requires": { "driverMin": "535.54", "gpuVendor": "NVIDIA", "computeCapabilityMin": 7.5, "dependsOn": ["nvidia-cuda-12"] }
    }
  ]
}
```

El manifiesto se firma con **Ed25519** (`manifest.sig`). La clave pública va embebida en el binario. Un manifiesto sin firma válida se rechaza y la app funciona con el manifiesto semilla empaquetado.

> **Estado: la descarga está implementada, la firma no.** Reparto real de responsabilidades:
>
> - `su-models` cubre el registro, el manifiesto, los estados de caché y la verificación de `sha256`. Es quien sabe **qué** hace falta: `ModelStatus.download` publica la URL, el hash y el tamaño, y es `None` cuando el modelo ya está instalado, cuando el manifiesto apunta a un `localPath` o cuando no declara hash. **Sin hash no se ofrece descarga**: no habría forma de saber si el archivo que llegó es el modelo que dice ser.
> - **La descarga la hace la aplicación, no el sidecar** (`apps/desktop/src/main/models/downloader.ts`). Ver ADR-018 en `docs/03`: un cliente HTTP en Rust arrastraría TLS con código C, y el proxy del sistema, el directorio de datos del usuario y el progreso en la interfaz son cosas de la capa de aplicación. Node trae `fetch`, `crypto` y streaming sin añadir una dependencia.
> - El descargador escribe en `<destino>.part`, reanuda con `Range` si hay un parcial, comprueba el `sha256` y solo entonces renombra al nombre definitivo: nadie puede observar un modelo a medio escribir con el nombre bueno. Cancelar **conserva** el `.part`, que es lo que permite reanudar.
> - **La firma Ed25519 del manifiesto sigue sin implementar**, y `su-models` sigue sin dependencia de criptografía. La comprobación de `sha256` es obligatoria y es la que hoy sostiene la integridad.

### Campos relevantes para el tiling

| Campo | Significado |
|---|---|
| `tiling.candidates` | Tamaños de tile a probar, en orden descendente |
| `tiling.overlapDivisor` | `overlap = clamp(tile / divisor, 16, 64)` |
| `tiling.padTo` | Múltiplo al que se alinea cada tile (importante para TensorRT/DirectML) |
| `tiling.vramPerMegapixel` | Estimación inicial; se sustituye por el valor calibrado tras la primera ejecución |
| `tiling.msPerMegapixelCpu` | Para estimar tiempos en CPU antes de empezar |

**Estas pistas llegan al runner** (`BackendProvider::tiling_hints`, aplicadas por etapa con `RunnerConfig::with_hints`). La precedencia, de más a menos, es: **manifiesto → calibración del equipo → estimación por defecto**. El manifiesto manda porque es lo que declara el autor del modelo —quien sabe que un tile de 1024 le hace producir artefactos—, la calibración mide lo que ese modelo consume **en esta máquina** y el valor por defecto es el último recurso. Ver ADR-030.

> **Estado del catálogo.** Las entradas embebidas **no declaran todavía** secciones `tiling`: el mecanismo está implementado y probado, pero los valores hay que medirlos modelo a modelo, y el valor por defecto sigue siendo razonable. Un modelo instalado a mano con `localPath` puede declararlos en un manifiesto propio.

### Otras pistas del modelo

| Campo | Significado |
|---|---|
| `scale` | Factor nativo (1 para los que no escalan: denoise, cara, detector) |
| `autoPriority` | Menor = preferido en modo automático dentro de su `kind` |
| `license.commercialUse` | Si es `false`, la interfaz muestra el aviso **antes** de descargar |

---

## 3. Pipelines por modo

### 3.1 Fotos — 4x (cadena por defecto)

```jsonc
{
  "id": "photo:4x",
  "mode": "photo",
  "scale": 4,
  "stages": [
    { "id": "analyze", "op": "analyze" },

    { "id": "denoise", "op": "model", "model": "scunet-color",
      "when": "analysis.noise > 0.35 && prefs.denoise != 'off'",
      "blendFrom": { "path": "analysis.noise", "min": 0.5, "max": 1.0 } },

    { "id": "upscale", "op": "model", "model": "4x-ultrasharp",
      "fallbackModel": "realesrgan-x4plus",
      "tiling": "auto", "scaleOut": 4 },

    { "id": "face", "op": "model", "model": "gfpgan-v1.4",
      "when": "analysis.faces > 0 && prefs.faceRestore != 'off'",
      "onlyOnFaces": true, "blend": 1.0 },

    { "id": "sharpen", "op": "unsharp",
      "when": "prefs.sharpen && analysis.blockiness < 0.2",
      "amount": 0.35, "radius": 1.2, "threshold": 0.02 }
  ]
}
```

**Por qué este orden.** El denoise va **antes** del upscale: si se aplica después, el modelo ya ha amplificado el ruido y el denoise posterior elimina detalle sintetizado. La restauración facial va **después** del upscale porque los modelos faciales esperan caras de tamaño razonable, y se compone solo sobre las cajas detectadas (`onlyOnFaces`), con mezcla suave en los bordes de la máscara. El enfoque va al final y se inhibe en imágenes con artefactos de bloque JPEG (`blockiness` alto), donde solo amplificaría los bordes de bloque.

**El peso del denoise depende de lo que se ha medido.** `blendFrom` interpola entre `min` (variable a 0) y `max` (variable a 1), con la variable recortada a `0..1`: con ruido moderado el peso ronda 0.5, que conserva el detalle, y con ruido alto llega a 1.0, que limpia de verdad. Antes era un `blend: 0.9` fijo para cualquier ruido. Una ruta que no exista, o que no sea numérica, es un **error**, no un cero silencioso. Ver ADR-031.

> **La etapa de denoise no se ejecuta hoy en producción**: su modelo (`scunet-color`) no tiene URL en el catálogo porque los únicos exports a ONNX reparten los pesos en dos archivos. El peso variable actúa cuando el equipo está en modo degradado, y el mecanismo queda listo para cuando el modelo se instale a mano con `localPath`.

### 3.2 Dibujo/Anime — 4x

```jsonc
{
  "id": "illustration:4x",
  "mode": "illustration",
  "scale": 4,
  "stages": [
    { "id": "analyze", "op": "analyze" },

    { "id": "lineclean", "op": "model", "model": "realesrgan-x4plus-anime-6b",
      "when": { "or": [
        { "path": "analysis.blockiness", "op": "gt", "value": 0.25 },
        { "path": "analysis.noise", "op": "gt", "value": 0.4 } ] },
      "blend": 0.7 },

    { "id": "upscale", "op": "model", "model": "realesrgan-x4plus-anime-6b",
      "tiling": "auto", "scaleOut": 4 },

    { "id": "sharpen", "op": "unsharp",
      "when": { "path": "prefs.sharpen", "truthy": true },
      "amount": 0.18, "radius": 0.8, "threshold": 0.03 }
  ]
}
```

Sin restauración facial (no aplica). El "suavizado de artefactos" se implementa como un enfoque suave (unsharp con umbral alto), que realza líneas sin amplificar el ruido de compresión. El pre-paso `lineclean` solo se activa cuando el análisis detecta compresión o ruido: en ilustraciones limpias sería contraproducente.

> **Corregido.** `lineclean` declara `scaleOut: 1`, así que el runner reduce la salida x4 del backend al tamaño de la entrada antes de mezclarla con `blend: 0.7`: `illustration:4x` devuelve 4x, no 16x. El arreglo necesitó dos cambios a la vez, y ambos están hechos. En el runner, la comprobación `backend.scale() != scale_out` se sustituyó por una reducción de la salida del backend a la escala declarada: una etapa puede pedir **menos** de lo que da su modelo (nunca más, eso es un error de autoría y sigue siéndolo). Y `blend` está implementado: la etapa de restauración mezcla píxeles en lugar de sustituirlos, y rechaza mezclar dos imágenes de tamaños distintos en vez de recortar en silencio.

### 3.3 Variantes 2x y 8x

```jsonc
{ "id": "photo:2x", "mode": "photo", "scale": 2,
  "stages": [ /* … upscale 4x … */
    { "id": "downscale", "op": "resize", "kernel": "lanczos3", "factor": 0.5 } ] }

{ "id": "photo:8x", "mode": "photo", "scale": 8,
  "stages": [ /* … upscale 4x … */
    { "id": "halve", "op": "resize", "kernel": "lanczos3", "factor": 0.5 },
    { "id": "upscale2", "op": "model", "model": "4x-ultrasharp",
      "tiling": "auto", "scaleOut": 4, "overlapBoost": 2 } ] }

{ "id": "illustration:2x", "mode": "illustration", "scale": 2,
  "stages": [ { "id": "upscale", "op": "model", "model": "2x-animesharpv3",
                "fallbackModel": "realesrgan-x4plus-anime-6b", "scaleOut": 2 } ] }
```

**`kernel` hace lo que dice, y el reescalado va en coma flotante.** Hasta ADR-025 el campo se ignoraba (todo se hacía con Lanczos3) y el paso por `image` cuantizaba a 8 bits por canal, aunque el ejemplo de arriba llegó a documentar `"precision": 16`. Ahora `resize` construye una imagen `Rgb32F`/`Rgba32F`, reescala en `f32` y acepta `lanczos3`, `catmullrom` (o `bicubic`, que es el nombre popular de la familia), `gaussian`, `triangle` y `nearest`. Un kernel desconocido es un error con la lista de válidos, no un valor por defecto silencioso. También se conservan el perfil ICC y la orientación, que antes se perdían en cada etapa de reescalado.

**La reducción intermedia del 8x no es opcional.** Dos pasadas de un modelo x4 dan 16x, no 8x. La etapa `halve` es lo que hace que `4 × 0.5 × 4 = 8`. Un test de `su-core` (`every_pipeline_lands_on_the_scale_it_promises`) comprueba que el producto de los factores de cada pipeline coincide con la escala de su identificador, así que un pipeline que se equivoque aquí no llega a compilar los tests.

`overlapBoost: 2` duplica el solape en la segunda pasada de 8x, porque cada costura de la primera pasada se amplifica ×4 en la segunda.

**El límite de megapíxeles se comprueba antes de empezar, no a mitad de cadena.** `upscale2` no lleva `when`: si el 8x no cabe en `maxOutputMp`, el runner rechaza el trabajo con `SU-E142` (`OutputTooLarge`) antes de decodificar nada, en lugar de omitir la etapa y devolver un 2x con la etiqueta de 8x. Como red de seguridad, al terminar se comprueba que el resultado mide lo que promete el pipeline; si no, se devuelve `SU-E143` (`ScaleNotReached`) con las etapas omitidas como contexto. Entre las dos comprobaciones, AC-04 se cumple sin que una cadena pueda mentir sobre su escala.

### 3.4 Cadena efectiva vs cadena declarada

Las etapas con `when` falso no se ejecutan. El sidecar reporta la cadena real en dos campos de `JobItem`: `effectivePipeline`, con las etapas que sí se ejecutaron, y `skipped`, con las que no y **por qué** (`"etapa: motivo"`). Los dos se persisten con el trabajo, así que siguen ahí al recargar la cola y no dependen de que un cliente esté conectado en el momento justo. Esto es clave para la depuración: "¿por qué esta imagen salió distinta?" tiene una respuesta visible.

---

## 4. Selección automática de modelos

Modo **Automático (recomendado)** — lógica de decisión:

```
1. kind_efectivo = analysis.kind si confidence > 0.80, si no el elegido por el usuario
2. Si modelOverride está definido → usar override (modo Manual)
3. Base:
     kind_efectivo == photo         → "4x-ultrasharp"  (fallback "realesrgan-x4plus")
     kind_efectivo == illustration  → "realesrgan-x4plus-anime-6b"
4. Ajustes por análisis:
     noise > 0.35                     → activar etapa "denoise" (scunet-color)
     blockiness > 0.25                → NO activar sharpen (amplificaría artefactos)
     faces.count > 0 && photo         → activar etapa "face" (gfpgan-v1.4)
     (hoy faces.count siempre es 0: el detector no está implementado, así que
      la etapa facial se omite con su motivo en lugar de fingir que corre)
5. Ajustes por hardware:
     VRAM_libre < 3 GB                → tile máx 384, unloadBetweenImages = true
     EP == CPU                        → aviso de tiempo estimado; sugerir tile 256
     resolución > 12 MP && scale == 8 → aviso de tiempo/espacio; ofrecer 4x
6. Ajustes por escala: ver ADR-008
```

Modo **Manual:** el usuario elige modelo base, denoise sí/no, restauración facial e intensidad, y puede editar la cadena. Las elecciones manuales se guardan por modo en los ajustes y se restauran en la siguiente sesión. El modo Manual **no** desactiva las salvaguardas (tiling adaptativo, validación de salida, degradación progresiva): esas no son configurables.

---

## 5. Análisis pre-upscaling en detalle

| Etapa | Implementación | Coste típico (2 MP) |
|---|---|---|
| Reducción de trabajo | Lado mayor ≤ 1024 px | ~8 ms |
| Orientación EXIF | `kamadak-exif` + transformación | ~2 ms |
| Ruido σ | Estimador wavelet (Donoho–Johnstone) sobre luma | ~6 ms |
| Blockiness | Varianza de diferencias en la rejilla 8×8, normalizada | ~4 ms |
| Tipo de contenido | 6 heurísticas + clasificador MobileNetV3-Small (224×224) | ~25 ms (CPU) |
| Rostros | YuNet 320×320 — **no implementado** (ver abajo) | — |
| Resolución/alfa | Cabecera del archivo | < 1 ms |

**Total:** ~60 ms por imagen. Para lotes de más de 200 imágenes, el análisis se ejecuta en el pool de E/S en paralelo con la inferencia del item anterior, de modo que no añade latencia perceptible al lote.

> **Estado real de las dos últimas filas.** Ni la clasificación de contenido ni la
detección de rostros están implementadas: `su-analyze` mide ruido, artefactos,
> resolución, alfa y orientación, y deja `kind` en `Unknown` con confianza 0 y
> `faces` **vacía**. Las dos filas de la tabla son el diseño previsto, no lo que se
> ejecuta hoy. La consecuencia visible es que la etapa facial se omite con el motivo
> «no se detectaron caras» y que el pipeline nunca contradice al usuario sobre el
> tipo de imagen. La parte del motor que sí está hecha (recortar, restaurar y pegar
> cada cara con máscara) está documentada en ADR-032; lo que falta es el detector
> (`yunet-2023`), que **no está en el catálogo embebido**.

### Lo que hace la etapa `onlyOnFaces` (ADR-032)

Cuando una etapa de modelo declara `"onlyOnFaces": true`, el motor no pasa la imagen
por el modelo:

1. Toma las cajas de rostro fiables que le da el análisis (`analysis.faces`) y
   **fusiona** las que se solapan más de la mitad del área menor.
2. Por cada caja, recorta un cuadrado centrado en ella con un 35 % de margen, lo
   desplaza para que quepa entero en la imagen y lo lleva a **512×512**, que es el
   tamaño con el que se exporta GFPGAN.
3. Ejecuta el modelo sobre el recorte.
4. Lo pega de vuelta con una **máscara radial** que vale 1 en todo el rostro y cae a
   0 en el borde del recorte, con el peso de `prefs.faceRestore` (0.6 suave, 0.85
   automático, 1.0 alta) multiplicado por el `blend` de la etapa. En los pipelines
   embebidos ese `blend` es **1.0** para que la preferencia del usuario sea la que
   decide.
5. Una cara con menos de 96 px de recorte se deja como estaba, y la nota dice
   cuántas quedaron fuera. Sin cajas, la etapa se reporta como omitida con «no se
detectaron caras».

La etapa debe ser de escala 1 (`scaleOut: 1` o un modelo sin marca de escala en su
identificador): una etapa "solo sobre caras" que además escalara no tendría un
tamaño de destino sensato para el pegado, y el motor lo rechaza con un error en
lugar de adivinar.

### Estimación de tiempo antes de empezar

```
ms_por_mp = calibrations[(modelo, ep, dispositivo)]?.msPerMegapixel
         ?? manifiesto.tiling.msPerMegapixelCpu   (si EP == CPU)
         ?? valor conservador por arquitectura
tiempo_item = ms_por_mp × megapíxeles_salida / 1000
tiempo_lote = Σ tiempo_item / concurrencia  + overhead_fijo × nº_items
```

Se muestra como rango ("entre 4 y 7 minutos") y se refina con el primer item completado. La calibración mejora con el uso, así que las estimaciones son cada vez más precisas.

---

## 6. Test de conformidad de modelos

Todo modelo, antes de entrar en el manifiesto oficial, pasa `su-cli model verify <id>`:

1. **Carga** en CPU EP: sin errores de grafo ni opcodes desconocidos.
2. **Forma:** entrada `1×3×64×64` → salida `1×3×256×256` (para x4).
3. **Rango:** salida dentro de `[-0.5, 2.0]` (margen amplio); si aparece `NaN`/`Inf`, se rechaza.
4. **No degenerado:** la salida no es constante ni uniforme.
5. **Paridad numérica:** si se conoce la implementación de referencia, `max|diff| < 1e-3` en fp32.
6. **Calidad:** PSNR y SSIM sobre el set de referencia sintético (bicúbico degradado → restaurado) superan el umbral mínimo del `kind`.
7. **Carga en cada EP disponible** y registro de tiempos.
8. **Determinismo:** dos ejecuciones con la misma entrada producen la misma salida (tolerancia 1e-5).

El resultado se guarda en `cache/model-verification.json` y es un requisito de CI para los modelos del manifiesto semilla.

---

## 7. Cómo añadir un nuevo modelo

**Sin tocar código.** Tres pasos:

1. **Convertir a ONNX** siguiendo §1 y verificar paridad numérica.
2. **Calcular el hash y el tamaño:**
   ```bash
   sha256sum mi-modelo.onnx
   ```
3. **Añadir la entrada al manifiesto** (`models/manifest.json` o, para uso personal, `<appData>/models.user.json`, que tiene prioridad):
   ```jsonc
   {
     "id": "mi-modelo-4x",
     "name": "Mi Modelo 4x",
     "kind": "photo",            // photo | illustration | denoise | face | detector | classifier
     "scale": 4,
     "sha256": "<hash>",
     "sizeBytes": 12345678,
     "urls": ["https://…/mi-modelo.onnx"],
     "license": { "name": "MIT", "commercialUse": true },
     "precision": { "fp16Safe": false },
     "tiling": {
       "candidates": [768, 512, 384, 256],
       "overlapDivisor": 16,
       "padTo": 32,
       "vramPerMegapixel": 500.0
     },
     "tags": ["mi-modelo"]
   }
   ```

El modelo aparece automáticamente en el selector del modo Manual. Para que sea candidato en modo Automático, hay que marcarlo con `"autoPriority": <número>` (menor = preferido) dentro de su `kind`.

**Recomendación de calibración:** si no se conoce `vramPerMegapixel`, poner un valor alto (conservador). La primera ejecución lo recalibra automáticamente y lo guarda en `cache/calibration.json`.

**Las pistas de `tiling` se usan de verdad** (ADR-030): deciden los tamaños de tile que ese modelo admite, el divisor del solape, la alineación y su VRAM por megapíxel de tile. Si un modelo produce artefactos con tiles grandes, se declaran `candidates` por debajo de ese tamaño y el motor no los superará; si no se declara nada, se usan los valores por defecto.

**Para añadir un detector de rostros** (lo que hoy falta para que la restauración facial funcione en producción, ADR-032) hacen falta dos cosas: la entrada en el manifiesto con `"kind": "detector"`, `"scale": 1` y, si se puede descargar, URL y `sha256`; y la implementación del análisis, que hoy devuelve `faces` vacía. El motor ya consume las cajas (`FaceBox` normalizadas `0..1` con confianza) sin cambios.

**Modelos locales sin descarga:** si el archivo ya está en `models/`, se puede omitir `urls` y usar `"localPath": "models/mi-modelo-4x.onnx"`. El hash se verifica igualmente.

---

## 8. Estado de disponibilidad de los modelos

| Modelo | Modo | Estado | Nota |
|---|---|---|---|
| ONNX identidad x4 (generado) | test | ✅ planificado (Fase 2) | Permite testear toda la tubería sin GPU ni descargas |
| `4x-ultrasharp` | Fotos | ✅ URL + `sha256` verificados | **CC-BY-NC-SA-4.0: no comercial** |
| `realesrgan-x4plus` | Fotos | ✅ URL + `sha256` verificados | Export de terceros; los pesos originales son BSD-3 |
| `realesrgan-x4plus-anime-6b` | Anime | ✅ URL + `sha256` verificados | BSD-3, comercial |
| `2x-animesharpv3` | Anime | ✅ URL + `sha256` verificados | **CC-BY-NC-SA-4.0: no comercial** |
| `scunet-color` | Denoise | ⛔ sin descarga | Ver abajo: el único export a ONNX reparte los pesos en dos archivos |
| `gfpgan-v1.4` | Cara | ✅ URL + `sha256` verificados | 340 MB: el más pesado del catálogo |
| `codeformer` | Cara | ⏳ no está en el catálogo embebido | S-Lab License: restricción comercial visible |
| `yunet-2023` | Análisis | ⏳ no está en el catálogo embebido | 337 KB. **Es lo que bloquea la restauración facial**: sin cajas, la etapa facial se omite con su motivo (ADR-032) |
| `content-classifier-v1` | Análisis | ⏳ por entrenar/verificar | Alternativa: solo heurísticas (funciona sin el modelo) |

Los hashes son los que publica el propio alojamiento (el `lfs.oid` de HuggingFace) y se comprobaron contra el `content-length` del archivo. Un test de `su-cli` (`every_downloadable_model_declares_a_verifiable_hash`) impide que una entrada con URL se quede sin hash, sin tamaño o sin TLS.

**Dos advertencias que no conviene perder de vista:**

- Solo `4x-ultrasharp` y `2x-animesharpv3` son exports del autor original. Los demás son conversiones de terceros, así que la identidad del modelo se apoya en el nombre del archivo y en el hash, no en una cadena de custodia. El hash garantiza que el archivo no cambia; no garantiza que sea el mejor export de ese modelo.
- Los dos modelos no comerciales están en el camino por defecto de sus modos. Una aplicación MIT que los descargue por defecto y avise en la interfaz es lo que se puede hacer sin redistribuirlos, pero conviene tenerlo presente.

**`scunet-color` no tiene descarga a propósito.** Los únicos exports a ONNX que existen reparten los pesos entre `.onnx` y `.onnx.data`, y el descargador trae un archivo suelto. Se deja declarado en el catálogo para poder instalarlo a mano con `localPath`, que ya está soportado, en lugar de ofrecer un botón que fallaría al cargar el modelo.

---

## 9. Estrategia de fallback de modelos

Lo que está implementado hoy, y lo que no:

```
1. Modelo preferido del pipeline                              ✅
2. → fallbackModel declarado en el pipeline                   ✅ desde ADR-025
3. → otro modelo instalado del mismo kind y scale             ❌ pendiente
4. → preguntar al usuario antes de sustituir                  ❌ pendiente
5. → si no hay ninguno: SU-E110 con acción "abre el gestor
     de modelos y descárgalo"                                 ✅
```

Los pasos 1, 2 y 5 son reales. El **2 llevaba desde el primer día declarado en los seis pipelines embebidos, documentado aquí y sin leer en el runner**: si el usuario instalaba el sustituto y no el principal, la imagen fallaba con «modelo no disponible» teniendo al lado uno que sirve.

Lo que sí está cerrado es que la sustitución **nunca es silenciosa**: aparece en el resumen final del lote. Hasta ADR-025 tampoco eso llegaba al usuario —el motor calculaba los motivos y la interfaz los descartaba—, así que ahora el resumen final muestra, por imagen, qué etapa se omitió y con qué modelo se sustituyó.

### Cuando falta un modelo de una etapa que no escala

Un modelo ausente **no tumba la imagen** si la etapa no aporta escala. El criterio es `scaleOut` (o, si la etapa no lo declara, la escala nativa que el manifiesto declara para el modelo):

- `lineclean` declara `scaleOut: 1`: si falta su modelo, la etapa se omite, se anota el motivo y el escalado sigue. Es el caso de `scunet-color` (denoise) y `gfpgan-v1.4` (rostros), y también una etapa `onlyOnFaces` **sin cajas que recortar**.
- `upscale` declara `scaleOut: 4`: si falta su modelo y no hay reserva, la imagen **falla** con `SU-E110`. Devolver otra medida sería peor que no devolver nada.

Antes, sin esta regla, una imagen ligeramente ruidosa en modo Fotos fallaba en la reducción de ruido opcional —cuyo modelo no se puede descargar— aunque el modelo de escalado estuviera instalado.

### El canal alfa y las etapas (ADR-029)

Al escribir un `pipelines.user.json` conviene saber qué le pasa a la silueta de una imagen con transparencia, porque **no todas las etapas la llevan**:

- **La llevan las etapas que cambian la geometría**: las que declaran `scaleOut > 1` y las de op `resize` con un `factor` distinto de 1. En esas, el canal alfa pasa por el mismo modelo, el mismo tile y el mismo solape que el color; es la única forma de que el contorno quede tan afilado como el interior.
- **No la llevan las etapas de restauración** (las que no cambian el tamaño, según la regla de la sección anterior). Usan un modelo x4 y devuelven la imagen a su tamaño: el reescalado de vuelta volvería a difuminar el contorno, y además cuesta una inferencia de más.
- **`blend` no se aplica al alfa.** Mezclar la salida de un restaurador suaviza el color, que es lo que se busca; la silueta no es un color que se mezcle, es la forma de la imagen.
- En una imagen **sin** canal alfa no hay pasada extra: el coste de reconstruir la silueta solo lo pagan los PNG con transparencia, y es una pasada de inferencia más por etapa de escalado.
