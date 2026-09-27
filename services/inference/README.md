# Sidecar de inferencia

Backend de SuperUpscaly: escala imágenes con ONNX Runtime y aceleración por
hardware. Se comunica con la interfaz por HTTP y WebSocket en loopback.

## Requisitos

- Rust estable (probado con 1.98). Para compilar y ejecutar los tests **no hace
  falta nada más**: el workspace no depende de ONNX Runtime.
- Para el escalado con modelos reales, activar la feature `onnx` y aportar el
  binario de ONNX Runtime.

## Primeros pasos

```bash
cargo test                       # 344 tests, sin GPU, sin modelos y sin ORT
cargo test --workspace --all-features   # 363, incluidos los de ort_backend
cargo run -p su-cli -- capabilities
cargo run -p su-cli -- pipelines
cargo run -p su-cli -- models
```

`su-cli upscale` funciona recién clonado el repositorio, sin descargar nada:
usa el backend de referencia, que reescala con la geometría exacta del modelo
que sustituye. Sirve para verificar el flujo completo, no la calidad del
resultado.

```bash
cargo run -p su-cli -- upscale foto.png --output ./salida --scale 4
cargo run -p su-cli -- upscale *.png --output ./salida --mode illustration --scale 2
cargo run -p su-cli -- serve --port 0 --portfile ./runtime.json
```

### Con ONNX Runtime

```bash
cargo build -p su-cli --features onnx
```

El crate usa `load-dynamic`: **no enlaza ORT en tiempo de compilación**. El
binario de ONNX Runtime (y las bibliotecas del execution provider, si aplica) se
cargan en tiempo de ejecución desde el directorio del ejecutable. Eso es lo que
permite distribuir un instalador base pequeño y ofrecer los aceleradores de
NVIDIA como descarga aparte (ADR-012).

Si el EP elegido no arranca, el error es explícito: `su-cli capabilities` informa
de qué proveedores están disponibles y por qué no lo están los demás.

## Arquitectura en una vista

```
su-cli / su-server            <- interfaz de entrada
        |
      su-jobs                 <- cola, pausa/cancelacion, eventos
        |
   su-analyze  su-inference   <- analisis previo y ejecucion de pipelines
        |            |
        |      su-tiling      <- planificacion de tiles y VRAM
        |            |
   su-imageio   su-models     <- E/S de imagen y catalogo de modelos
        \           /
          su-core             <- dominio, errores y motor de pipelines
        su-telemetry          <- logs
```

`su-inference` define el trait `Backend` y hay tres implementaciones: `MockBackend`
(vecino más cercano, para verificar geometría y composición sin ONNX Runtime, ver
ADR-018), `ClassicalBackend` (interpolación en `f32`, el motor que corre cuando no
hay runtime o no hay modelos) y `OrtBackend`, la inferencia real, **detrás de la
feature opcional** `onnx`.

Cuando un modelo falta, el trabajo no se pierde: `JobContext.fallback` permite
reintentar una vez con la interpolación y el item queda marcado como degradado con
el motivo escrito (ADR-026).

## Estructura

| Crate | Responsabilidad |
|---|---|
| `su-core` | Tipos de dominio, taxonomía de errores y motor de pipelines declarativos |
| `su-tiling` | Planificación de tiles, presupuesto de VRAM, composición sin costuras |
| `su-imageio` | Decodificación, orientación EXIF, escritura atómica, validación de salida |
| `su-models` | Manifiesto, verificación SHA-256 y estado del caché local |
| `su-hardware` | Detección de CPU y GPU, inventario de execution providers |
| `su-analyze` | Estimación de ruido y artefactos de compresión |
| `su-inference` | Abstracción `Backend`, runner de pipelines, degradación progresiva |
| `su-jobs` | Cola de trabajos, pausa y cancelación, flujo de eventos |
| `su-server` | API HTTP + WebSocket en loopback con token |
| `su-telemetry` | Logs JSON con enmascarado del directorio personal |
| `su-cli` | Interfaz de línea de comandos |

## API

Todas las rutas salvo `/v1/health` exigen `Authorization: Bearer <token>`.

| Método | Ruta | Descripción |
|---|---|---|
| GET | `/v1/health` | Liveness. Sin autenticación |
| GET | `/v1/capabilities` | Hardware y execution providers disponibles |
| GET | `/v1/models` | Catálogo y estado de cada modelo |
| GET | `/v1/pipelines` | Pipelines cargados |
| POST | `/v1/jobs` | Crea un trabajo |
| GET | `/v1/jobs` | Lista de trabajos |
| GET | `/v1/jobs/{id}` | Estado detallado de un trabajo |
| POST | `/v1/jobs/{id}/pause` · `/resume` · `/cancel` | Control de ejecución |
| GET | `/v1/events` | WebSocket con el flujo de eventos |
| POST | `/v1/shutdown` | Cierre ordenado |

## Problemas conocidos al compilar

**Errores de API en `image` o `axum`.** Estaban previstos por escribirse el
workspace en un entorno donde no se podía compilar, y no apareció ninguno: el
workspace compila sin avisos con Rust 1.98 y las 363 pruebas pasan. Si vuelven a
aparecer tras actualizar esas crates, son errores de compilación claros y
localizados: el mensaje indica la línea.

**`error: could not exec the linker` o `dlltool: program not found`.** Falta
binutils. Ocurre solo con el target `x86_64-pc-windows-gnu`, que necesita
`dlltool` para enlazar con `windows-sys`. En Linux y macOS no aplica.

**`Access denied` al compilar.** El proyecto evita a propósito dependencias que
arrastren `windows-sys` (`num_cpus`, `sysinfo` y `tempfile` se sustituyeron por
la biblioteca estándar) porque el target GNU no trae el enlazador necesario.

## Cómo añadir un modelo

Ver `docs/04-modelos-y-pipelines.md`. En resumen: convertir a ONNX, calcular el
SHA-256, añadir la entrada al manifiesto. No hace falta tocar código.
