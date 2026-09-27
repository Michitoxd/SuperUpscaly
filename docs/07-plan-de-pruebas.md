# 07 · Plan de pruebas

> Qué se comprueba automáticamente, qué hay que comprobar a mano, y **cómo
> reproducir los fallos** que este proyecto existe para eliminar.

---

## Pruebas automáticas

### Sidecar (Rust)

```bash
cd services/inference
cargo test
```

344 pruebas (363 con `--all-features`: las de `ort_backend` van detrás de la
feature `onnx`, que necesita el binario de ONNX Runtime). **No necesitan GPU, ni
modelos, ni ONNX Runtime.** Es posible porque la inferencia pasa por el trait
`Backend` y los tests usan el backend de referencia (ADR-018).

```bash
cargo test --workspace --all-features    # incluye los tests de ort_backend
```

Estas pruebas **se ejecutan de verdad** en el equipo de desarrollo. La primera
vez que se ejecutaron encontraron siete defectos de producción: ver ADR-022.

Qué cubren, por crate:

| Crate | Qué verifica |
|---|---|
| `su-tiling` | Que los pesos coseno suman exactamente 1 en los solapes; que cada píxel queda cubierto con peso 1; que la geometría sobrevive al troceado; que la degradación nunca devuelve un tile mayor |
| `su-core` | Que el producto de los factores de cada pipeline coincide con su escala; que una variable mal escrita en una condición es un error, no un `false`; que el peso variable (`blendFrom`) interpola entre sus extremos y que una ruta mal escrita es un error; que los tres pipelines de foto hacen crecer el peso del denoise con el ruido |
| `su-imageio` | Que un fallo de validación **no crea el archivo destino**; que la escritura atómica no deja temporales; que la orientación EXIF se aplica; que el relleno de transparencia solo toca los píxeles con alfa cero y deja el alfa intacto; que la máscara radial se anula en todo el contorno y que el pegado mezclado no toca lo que la máscara no cubre; que la uniformidad se mide sobre luma Rec. 709 (un rojo plano se rechaza) |
| `su-inference` | Que el peso del denoise cambia el resultado según el ruido medido; que las cajas de cara que se solapan se funden y que el recorte cabalga sobre el borde de la imagen sin salirse; que la restauración facial cambia el rostro, deja el resto intacto y mezcla el margen a un valor intermedio |
| `su-jobs` | Que un trabajo interrumpido vuelve en pausa; que reanudar no reprocesa lo ya hecho ni cuenta dos veces los fallos; que sin el modelo de la etapa de escalado la imagen **se interpola y se dice** en lugar de perderse, y que sin respaldo en el contexto el fallo sigue siendo el de antes |
| `su-hardware` | Que un `nvidia-smi` mal formado no pierde las GPU buenas; que un EP sin su biblioteca se reporta como no disponible con motivo |

```bash
# Con la implementacion de ONNX Runtime (necesita el binario de ORT)
cargo build --features onnx -p su-cli
```

### Integración (Rust)

```bash
cd services/inference && cargo test -p su-server --test api
```

11 pruebas que levantan el servidor **de verdad**, en un puerto efímero, y hablan
con él por un socket escribiendo la petición HTTP a mano. Las pruebas unitarias
del crate llaman al router en memoria, así que no ven ni el enlace del puerto, ni
el fichero de puerto, ni el middleware de autorización sobre una conexión real —
y esas tres cosas son justo lo que la aplicación usa para hablar con el motor.
Cubren: la salud sin token, que **todo lo demás exija token** (y rechace uno
equivocado), las capacidades, el `modelsDir` que la aplicación necesita para
descargar, la lista de pipelines, el ciclo crear/listar trabajo, y que un trabajo
vacío se rechace con su código `SU-E001` hasta el cliente.

Tres de ellas existen por un defecto concreto (ADR-024): que las **rutas con
identificador lleguen al manejador** en lugar de morir en el enrutador, que un
trabajo creado se pueda **leer, pausar, reanudar y cancelar** por su id, y que
controlar un trabajo ya terminado no se reporte como error interno. La primera
distingue el `404` de «esa ruta no existe» del `405` de «ese método no vale aquí»:
**una prueba que solo comprueba que la respuesta no es un 500 la satisface un 404
y por tanto no prueba el enrutador.**

No se usa ningún cliente HTTP: cuatro líneas sobre un `TcpStream` y se evita
arrastrar una dependencia entera solo para probar.

### Interfaz (TypeScript)

```bash
npm run typecheck      # los cuatro paquetes
npm test               # 92 tests: scripts del harness, CSP, descarga y garantía de modelos, ZIP/CBZ, expansión en la lista, el runtime y el registro de imágenes
```

`npm test` ejecuta Node con `--test` sobre TypeScript directamente (Node 22 quita
los tipos). Cubre `scripts/lib/stats.test.mjs` (16),
`apps/desktop/src/main/models/downloader.test.ts`,
`apps/desktop/src/main/models/ensure.test.ts`,
`apps/desktop/src/main/archives/reader.test.ts`,
`apps/desktop/src/main/sidecar/ort.test.ts`,
`apps/desktop/src/main/csp.test.ts` y
`apps/desktop/src/main/media/registry.test.ts`; los dos del medio levantan recursos
de verdad — servidores HTTP en puertos efímeros y archivos ZIP escritos byte a
byte — para comprobar la reanudación con `Range`, el rechazo por hash, el cambio
de espejo, la cancelación, el CRC de cada entrada y el orden de las páginas.

Los ZIP de prueba se **construyen a mano** en el propio test en lugar de usar uno
de ejemplo: si se crearan con una herramienta y se leyeran con el mismo tipo de
herramienta, un malentendido del formato compartido por las dos pasaría
desapercibido.

El registro de imágenes que sirve la comparación (ADR-035) se prueba por lo que
**decide**, no por lo que sirve: qué rutas quedan autorizadas, que la coincidencia
sea exacta (una ruta parecida no vale), que una ruta sin forma segura no entre y que
la URL del contrato se reconstruya entera con espacios, acentos, almohadilla e
interrogación, y que una URL de otro origen o de otro esquema no se interprete como
petición de imagen. Es la prueba que además obligó a que el paquete compartido se
pueda cargar en ejecución (ADR-036): antes solo se importaban sus **tipos**, que
desaparecen al borrar los tipos y por eso nunca se intentaba resolverlo.

El typecheck cubre la frontera con el sidecar: `packages/shared/src/sidecar.ts` es
el espejo de los tipos de Rust. Si un campo se llama distinto en los dos lados, no
compila.

### Rendimiento

```bash
npm run benchmark -- --fixtures tests/fixtures --repeats 5
```

Ver `tests/fixtures/README.md` para el conjunto de referencia. El informe sale en
`docs/benchmarks/`.

---

## Matriz de dispositivos

Mínimo para dar una versión por buena. **Cada fila es un equipo distinto**: probar
tres GPUs en la misma máquina no cubre lo mismo que probarlas en tres sistemas.

| # | Sistema | GPU | EP esperado | Qué comprueba |
|---|---|---|---|---|
| 1 | Linux x64 | NVIDIA (CC ≥ 7.5) | TensorRT | La ruta rápida y los Acceleration Packs |
| 2 | Linux x64 | AMD | CPU | Que la ausencia de EP no rompe nada |
| 3 | Linux x64 | ninguna (VM) | CPU | Que funciona sin GPU en absoluto |
| 4 | Windows 11 | NVIDIA | TensorRT | Rutas largas, `\\?\` |
| 5 | Windows 10 | AMD o Intel | DirectML | El EP de Windows sin NVIDIA |
| 6 | macOS 13+ | Apple Silicon | CoreML | `arm64`, entitlements, `titleBarStyle` |
| 7 | macOS 12 | Intel | CoreML | `x64` en un sistema antiguo |

En cada uno:

```bash
su-cli capabilities
```

Y comprobar que el EP recomendado es el esperado y que los no disponibles
**explican por qué**.

---

## Pruebas manuales

### 0. La ventana responde

**Paso obligatorio de cada entrega, y el primero.** Con la aplicación construida
(`npm run build && npm start`, no `npm run dev`):

| Caso | Resultado esperado |
|---|---|
| Cambiar de modo Fotos → Dibujo/Anime → Fotos | La marca de selección se mueve en los dos sentidos |
| Desplegar «Avanzado» | El panel se abre |
| Pulsar «Elegir imagen» | Se abre el diálogo nativo del sistema |
| Abrir el diálogo de modelos | Lista los modelos con su estado real |

Y en el registro (`~/.config/SuperUpscaly/logs`) no debe aparecer
`renderer.console-error`, ni `renderer.preload-error`, ni
`renderer.load-failed`.

Existe por un motivo concreto: una ventana que se ve pero no responde **no la
detecta ninguna prueba automática** —los tests de Node no cargan páginas en un
navegador— y tampoco se ve en desarrollo, donde el renderer no lleva CSP. Pasó una
vez (ADR-023) y este es el paso que lo habría cazado en el primer minuto.

### 0b. El trabajo se completa y se puede seguir

**Paso obligatorio de cada entrega.** Se hace justo después del anterior, con la
misma build:

| Caso | Resultado esperado |
|---|---|
| Arrastrar una imagen de 48×32 y pulsar «Upscaly» | Recorre sus etapas y termina en `Completado` |
| Mirar la carpeta de salida | El PNG está ahí, a la escala pedida (192×128), y abre bien |
| Pulsar «Abrir carpeta de salida» | Responde en ~1,5 s; **no** se queda colgado |
| Durante un trabajo, mirar la barra de progreso | No promete nada que no sea cierto (ni «simulado» ni ninguna fase) |
| Pulsar «Pausar» o «Cancelar» justo cuando el trabajo acaba | La respuesta es el estado real del trabajo. **Nunca** un error del motor (ADR-034) |

En el registro no debe aparecer `reply was never sent`, ni `SU-E5xx` en un trabajo
que terminó bien, ni ninguna línea que diga `shell.open-path-sin-respuesta` fuera
del caso de una carpeta abierta de verdad.

Cubre lo que faltaba: la creación del trabajo **nunca estuvo rota**, así que todas
las pruebas anteriores pasaban mientras `GET /v1/jobs/{id}` respondía 404 para
cualquier id y abrir una carpeta colgaba el manejador de IPC (ADR-024).

### 0c. El modelo que falta se descarga, y sin él no se pierde la imagen

**Paso obligatorio antes de cada entrega con cambios en el motor.** Con la
instalación recién clonada —o borrando el directorio de modelos— la primera imagen
es el caso que más veces se rompió:

```bash
rm -rf ~/.local/share/superupscaly/models     # el estado de una instalación nueva
```

| Caso | Resultado esperado |
|---|---|
| Arrastrar una imagen, elegir Dibujo/Anime a 4x y pulsar «Upscaly» | La barra dice «Preparando: descargando el modelo» con un porcentaje que avanza |
| Mirar `~/.local/share/superupscaly/models/` | Aparece `realesrgan-x4plus-anime-6b-<hash>.onnx` (18 MB) |
| Mirar el registro | `models.downloaded` con el modelo y sus bytes, y `models.ensured` con el modo y la escala |
| Esperar al final | El PNG sale a la escala pedida, y tanto su **color** como su **contorno** (el canal alfa) tienen una transición de 1–2 px, no de 6 |
| Repetir con el modelo ya instalado | No se descarga nada (`present`), y el trabajo empieza de inmediato |

Y el caso de no poder descargar (sin red, o un catálogo sin URL):

| Caso | Resultado esperado |
|---|---|
| Con el modelo borrado y sin red, pulsar «Upscaly» | El trabajo **se acepta** y termina marcado como degradado, con la nota «motor de respaldo 'clasico-…'» visible en la lista |
| Comprobar la salida | Hay PNG, con la calidad de un interpolador. El informe dice cuál de los dos motores corrió |

### 1. Arrastrar y soltar

| Caso | Resultado esperado |
|---|---|
| 10 archivos de golpe | Aparecen los 10 con su tamaño real |
| Una carpeta con subcarpetas | Se expande recursivamente |
| Una carpeta vacía | Aviso claro, no una cola vacía sin explicación |
| Arrastrar texto desde un navegador | Se ignora sin ruido |
| Arrastrar un archivo sin permisos | Aviso con el motivo |
| 5000+ archivos | Se truncan con aviso, no se cuelga |

**En Wayland**: el portal XDG puede no exponer la ruta. Debe aparecer el aviso de
`SU-E161` y funcionar el botón de seleccionar archivos. **Este es el fallo que más
gente reporta de otras herramientas.**

### 2. Lote largo

- 1000 imágenes de 1 MP.
- Pausar a mitad y reanudar: no debe repetir las ya hechas.
- Cancelar a mitad: debe detenerse en menos de 2 s y dejar el estado consistente.
- **Matar el proceso del sidecar** (`kill -9`): al relanzar, el lote debe aparecer
  **en pausa** y poder continuarse. Es el criterio AC-07.

### 3. Imágenes grandes

- 20 imágenes de ≥ 100 MP.
- Una imagen de 8192×8192 ×4 en una GPU de 4 GB.

**Nunca debe aparecer una imagen negra.** Si la memoria no llega, el sistema
degrada el tile y lo reporta. Si el resultado no pasa la validación, **no se
escribe el archivo**: un hueco en la carpeta de salida es preferible a un PNG
negro que parezca válido.

### 4. Recuperación ante fallos

| Escenario | Resultado esperado |
|---|---|
| Archivo corrupto en medio del lote | Ese ítem falla con código; el resto continúa |
| Disco lleno a mitad | `SU-E150` con la ruta; no se escriben archivos truncados |
| GPU desconectada (eGPU) | `SU-E131`, mensaje claro, sin cuelgue |
| Driver reiniciado | El sidecar se recupera o se reinicia con backoff |
| Carpeta de salida de solo lectura | Se avisa antes de empezar |

### 4b. La cadena que dibuja el panel es la del motor

**Paso obligatorio al tocar los pipelines o el panel avanzado.** El panel dibuja
las etapas y el modelo de cada una; la verdad está en `GET /v1/pipelines`. Cuadran
en las cinco combinaciones:

| Modo y escala | Lo que tiene que decir el panel |
|---|---|
| Dibujo/Anime 2x | Escalando · `2x-animesharpv3` (no el de 4x) |
| Dibujo/Anime 8x | **dos** etapas de escalado, con la reducción intermedia entre ellas |
| Dibujo/Anime 4x | Limpieza con `realesrgan-x4plus-anime-6b` y escalado con el mismo modelo |
| Fotos 4x | `scunet-color`, `4x-ultrasharp` y `gfpgan-v1.4` en su sitio |
| Cualquiera con el motor en interpolación | La cadena sigue siendo la del pipeline; el motor que corre se ve en la barra lateral |

Las etapas con condición se dibujan con un `·?`: pueden no ejecutarse y el panel no
promete lo contrario. Lo que pasó **de verdad** en una imagen está en sus notas.

Y dos comprobaciones de arranque, que es donde apareció el fallo (ADR-028):

| Caso | Resultado esperado |
|---|---|
| Abrir la ventana cuando el motor ya está listo | El gestor de modelos lista el catálogo **sin abrirlo a mano** |
| Reiniciar el motor desde la barra lateral | La lista y la cadena se vuelven a pedir |

### 5. Paridad visual

Comparar la disposición con Upscayl: barra lateral, zona de arrastre, botón
principal y configuración avanzada en el mismo sitio. Solo deben diferir en dos
cosas: el color y el selector de modo.

### 5b. El contorno de un PNG con transparencia

**Es el defecto que el usuario reportó como «el borde se ve demasiado
pixelado/borroso»**, y la comprobación que lo cierra (ADR-029). Necesita un PNG
con canal alfa —arte recortado, un logotipo, un dibujo con fondo transparente—:

Sirve cualquier PNG con alfa; para el caso del alfa **suave** hace falta uno con
una pluma o una sombra, no una silueta recortada a cuchillo. Los dos se crean en
cualquier editor, o con `convert -size 256x256 radial-gradient: …` si hay
ImageMagick a mano.

| Caso | Resultado esperado |
|---|---|
| Escalar el PNG y mirar el contorno ampliado, sobre fondo claro **y** oscuro | El borde de la silueta es un escalón de 1–2 px, no una rampa de 5 a 7 px con un halo blanco que se deshace en el fondo |
| Medir la rampa del alfa en la salida (python + numpy) | Entre 0 y 1 píxeles de transición, como la referencia de Upscayl |
| Comprobar la posición del borde | Donde cruzaba el alfa 127 sigue cruzándolo: el borde se afila, no se desplaza |
| Un PNG con un alfa **suave** (pluma, sombra, degradado radial) | El degradado sigue siendo un degradado: si se convierte en un escalón, el paso por el modelo se ha llevado por delante algo que sí debía ser suave |
| Escalar un JPG (sin canal alfa) | El tiempo por imagen no cambia: sin transparencia no hay segunda pasada |
| Un PNG transparente a 8x | El contorno aguanta las dos pasadas de escalado; el alfa acaba con el tamaño de la imagen, nunca con el de una etapa intermedia |

### 5c. La restauración facial (y lo que hoy no se puede comprobar)

**Paso obligatorio al tocar el análisis o la etapa facial.** La etapa `face` de los
pipelines de foto declara `onlyOnFaces`, y desde ADR-032 lo cumple: recorta cada
cara, la restaura y la pega con una máscara radial.

Lo que **sí** se puede comprobar hoy, con una foto que tenga caras:

| Caso | Resultado esperado |
|---|---|
| Foto con caras, modo Fotos a 4x, restauración facial en «Automática» | Las notas de la imagen dicen que la etapa facial se **omitió** con el motivo «no se detectaron caras» |
| La misma foto con `prefs.faceRestore` en «Desactivada» | La etapa se omite por su condición, con otro motivo distinto |

Ese resultado es correcto y es el estado actual: **el análisis todavía no detecta
rostros** (`su-analyze` devuelve la lista vacía desde la Fase 2), así que no hay
cajas que recortar. La etapa está implementada y probada con cajas sintéticas; lo
que falta es el detector (`yunet-2023`), que no está en el catálogo embebido.

Cuando el detector exista, la comprobación pasa a ser esta:

| Caso | Resultado esperado |
|---|---|
| Foto con una cara, restauración en «Alta» | Solo la zona de la cara cambia; el fondo queda igual, píxel a píxel |
| Mirar el borde de la cara ampliado | No hay un rectángulo pegado: la transición es un degradado |
| Bajar la preferencia a «Suave» | El mismo recorte, con la restauración a media intensidad |
| Una cara pequeña (menos de 96 px de recorte) | Se deja como estaba y la nota dice cuántas caras quedaron fuera |
| Dos caras muy juntas | Se fusionan y se restaura una sola zona, sin costura entre ellas |

### 5d. El peso del denoise sigue al ruido (sin modelo de denoise instalado)

La etapa de reducción de ruido de los pipelines de foto declara su peso como
`blendFrom`, entre 0.5 y 1.0 según `analysis.noise`. Su modelo (`scunet-color`) no
se puede descargar todavía, así que la comprobación directa requiere instalarlo a
mano con `localPath` (ver `docs/04`).

Mientras tanto, la propiedad está cubierta por pruebas automáticas: el mismo
pipeline sobre la misma imagen da resultados **distintos** con ruido 0.4 y con
ruido 0.95, y los tres pipelines de foto exigen que el peso crezca con el ruido.

### 5e. La comparación antes y después

Con una imagen ya escalada en la cola, pulsa el botón **⇔** de su fila. Comprueba,
en este orden:

1. Las dos imágenes se ven: la de la izquierda es el original y a la derecha está el
   resultado. Las medidas de cada lado (`128 x 128 px` y `512 x 512 px`) coinciden
   con las de los archivos.
2. La línea empieza centrada y **se arrastra desde cualquier punto** de la imagen, no
   solo desde el tirador.
3. Con el tirador enfocado (`Tab`), las flechas mueven la línea un 2 % y `Inicio` y
   `Fin` la llevan a los extremos. En los extremos se ve **una sola** de las dos
   imágenes, entera.
4. El tirador no se sale de la imagen: con una imagen apaisada, la franja a cuadros
   queda por encima y por debajo, y la línea no la invade.
5. Con un PNG de alfa (por ejemplo uno recortado con esquinas redondeadas), el borde
   duro y limpio se ve **a los dos lados** de la línea cuando la cruzas.
6. `Escape` cierra la vista; el botón «Abrir carpeta» abre la carpeta del resultado.
7. Escala la misma imagen otra vez sobre la misma carpeta y vuelve a abrir la
   comparación: se ve el **resultado nuevo**, no el anterior (es lo que fija
   `Cache-Control: no-store`; sin él, Chromium reutiliza la copia y la vista miente).

Queda comprobado también en la ventana real al construir la función, con arrastre de
ratón y teclado de verdad: la línea pasó de 50 a 25 con el ratón y a 31 con tres
flechas, las dos imágenes cargaron, y la página no registró ningún error.

---

## Pruebas de estrés

```bash
# 500 ciclos de arrastrar y soltar
# (script pendiente: necesita la aplicacion en marcha)
```

| Prueba | Criterio |
|---|---|
| 500 ciclos de arrastrar y soltar | 0 cierres inesperados, 0 rutas vacías sin aviso |
| Lote de 1000 imágenes | Se completa sin límite artificial |
| 20 imágenes de ≥ 100 MP | Sin OOM, sin archivo corrupto |
| Matar el sidecar 20 veces seguidas | Se reinicia siempre, se rinde tras 5 con aviso |

---

## Antes de publicar una versión

- [ ] `cargo test` en verde.
- [ ] `npm run typecheck` en verde.
- [ ] `npm run test:scripts` en verde.
- [ ] Benchmark ejecutado y publicado en `docs/benchmarks/`.
- [ ] Las siete filas de la matriz de dispositivos comprobadas.
- [ ] Las pruebas manuales de §0, §0b, §0c, §4b, §5b, §5c, §5d y §1 a §5 pasadas.
- [ ] Instaladores probados en máquina limpia, no en la de desarrollo.

---

## Lo que este plan no cubre

- **Calidad subjetiva.** SSIM y LPIPS son medidas; que una foto "se vea mejor" no
  lo mide ninguna. El informe de benchmark incluye la tabla por imagen para que se
  pueda revisar a ojo.
- **Casos raros de hardware.** Una matriz de siete equipos no cubre todas las
  combinaciones de driver y GPU existentes.
- **Modelos que aún no existen.** El plan de pruebas cubre el motor, no la calidad
  de cada modelo concreto que se añada en el futuro.
