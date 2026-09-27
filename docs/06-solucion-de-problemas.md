# 06 · Solución de problemas

> Guía ordenada por **síntoma**, no por causa. Empieza por lo que ves.

---

## Lo primero: los logs

Casi todo lo de esta guía se resuelve mirando el log. Están en:

| Plataforma | Ruta |
|---|---|
| Linux | `~/.config/SuperUpscaly/logs/` |
| Windows | `%APPDATA%\SuperUpscaly\logs\` |
| macOS | `~/Library/Application Support/SuperUpscaly/logs/` |

La aplicación tiene un botón **Exportar diagnóstico** en el panel de errores que
genera un ZIP con los logs, `capabilities.json` y `settings.json`. Las rutas de tu
directorio personal aparecen enmascaradas como `<home>`: puedes adjuntarlo sin
revisarlo antes.

El sidecar escribe además su propio log (`su-server-AAAA-MM-DD.log`) en la misma
carpeta.

---

## «La ventana se ve, pero no responde a nada»

Ni un clic hace nada, el motor dice siempre **Detenido** y la carpeta de salida se
queda en **Cargando…**. Es el síntoma exacto de una página que **no ha hidratado**:
lo que ves es el HTML que sirvió el servidor de desarrollo, sin un solo manejador
montado. No hay ningún error escrito, porque no ha fallado nada: simplemente no hay
código escuchando.

Solo ocurre en `npm run dev`, y la causa es **el origen de la ventana**. El cliente
de desarrollo de Next abre su socket de recarga contra el mismo origen desde el que
se sirvió la página, y su servidor rechaza esa negociación cuando el `Host` es una
IP: con `Host: 127.0.0.1:3456` no contesta al `upgrade`, y con
`Host: localhost:3456` responde `101 Switching Protocols`. Sin ese socket, el
cliente no termina de arrancar y la interfaz se queda pintada y sorda.

Desde [ADR-038](03-decisiones-adr.md) `npm run dev` carga la ventana por
`localhost` y `next.config.mjs` admite `127.0.0.1` como origen de desarrollo, así
que las dos direcciones funcionan. Si te vuelve a pasar:

1. Abre `http://localhost:3456/` en un navegador. Si ahí reacciona y en la ventana
   no, es esto.
2. Mira qué dirección tiene la ventana en las DevTools: debe ser `localhost:3456`.
3. Si `localhost` no resuelve —o resuelve a `::1` y Next no escucha ahí—, `dev.mjs`
   lo avisa y cae a la IP: en ese caso la interfaz puede quedarse sin hidratar, y el
   aviso del arranque lo dice con esas palabras.

La aplicación empaquetada (`npm start` y los instaladores) **no pasa por aquí**:
sirve el export estático por el esquema `app://`, sin cliente de desarrollo y sin
HMR.

---

## El motor de escalado

El estado del motor está **siempre visible** en la barra lateral. Si algo va mal
ahí, el botón "Upscaly" no hará nada y el motivo está en esa línea.

### "No disponible" — no se encontró el ejecutable

El binario del sidecar no está compilado. Compílalo:

```bash
cd services/inference
cargo build --release -p su-cli
```

La aplicación lo busca en `services/inference/target/release/su-cli` (o `debug/`
si no hay release). Si lo tienes en otro sitio:

```bash
export SU_SIDECAR_BIN=/ruta/a/su-cli
npm run dev
```

### "Con error" — el sidecar termina al arrancar

Mira el log del sidecar. Las dos causas más frecuentes:

**Falta una biblioteca de ONNX Runtime.** Si compilaste con `--features onnx`, el
binario de ORT tiene que estar junto al ejecutable. El crate usa `load-dynamic`:
no enlaza ORT al compilar, lo carga en tiempo de ejecución. Copia
`libonnxruntime.so` (o `onnxruntime.dll`) al lado de `su-cli`.

Ojo con la versión: `ort` 2.0-rc.13 espera **ONNX Runtime 1.28**. La 1.22 que
traen algunas distribuciones no sirve y el intento de carga falla con
`dlopen failed` o un desajuste de versión de API.

Desde ADR-025 esto **no impide trabajar**: el sidecar comprueba el runtime antes
de aceptar el primer trabajo, avisa por `stderr` y por el registro, y sigue con
interpolación clásica. Si ves resultados menos nítidos de lo esperado y en el
registro aparece `sin ONNX Runtime: motor clasico`, es esto. La barra lateral de
la interfaz muestra el motor en uso (`CPU`, `CUDA`, `clasico-catmullrom`).

Dónde busca la biblioteca, en este orden:

1. `ORT_DYLIB_PATH`, si está definida.
2. `<directorio de datos de la aplicación>/runtime/`.
3. Junto al ejecutable del sidecar.

En desarrollo, la aplicación le pasa la ruta al sidecar por ti; el argumento
`SU_DATA_DIR` permite además usar otro catálogo de modelos sin tocar el del
usuario:

```bash
SU_DATA_DIR=/tmp/pruebas npm start
```

### «El motor clásico no tiene la nitidez que esperaba»

Es esperable y está medido. Un interpolador no puede recuperar un borde duro de
un trazo de 1 px: reparte el escalón en una rampa de unos 4 px de origen.
Recuperarlo es deconvolución, y es justo lo que aprende un modelo entrenado con
dibujos. Instala el modelo del modo que uses (gestor de modelos de la interfaz) y
el motor cambiará solo: la barra lateral dejará de decir `clasico-catmullrom`.

Medido sobre una ilustración de color plano, el ancho de la transición del borde:
**6 px** con interpolación frente a **2 px** con el modelo de anime. Es la
diferencia que se ve como «borde borroso».

### «Puse restauración facial y las notas dicen "no se detectaron caras"»

Es el estado actual, no un fallo de la instalación. La etapa facial solo actúa
sobre las caras que el análisis detecta, y **el análisis todavía no detecta
rostros**: devuelve la lista vacía, así que la etapa se omite y lo dice. El motor
sí sabe recortar, restaurar y pegar cada cara (ver ADR-032); lo que falta es el
detector, que no está en el catálogo embebido.

Sí es un fallo que se pueda resolver cuando aparezca el aviso **«N cara(s)
demasiado pequeñas para restaurar»**: las caras con menos de 96 px de recorte se
dejan como estaban a propósito, porque a ese tamaño el modelo inventa rasgos en
lugar de recuperarlos.

### «Pausar justo cuando el trabajo termina»

Debe responder con el estado real del trabajo (terminado), nunca con un error del
motor. Hubo una ventana de milisegundos en la que podía responder un 500 si el
trabajo acababa entre la comprobación y la orden; está cerrada en ADR-034 y hay una
prueba que la busca a propósito (25 intentos con pausa y cancelación inmediatas). Si
ves un 500 ahí, es un fallo nuevo: adjunta el log.

### «El resultado sale interpolado aunque la descarga del modelo terminó bien»

Tres causas, por orden de frecuencia:

1. **El modelo que falta es de una etapa de restauración**, no de la de escalado
   (por ejemplo `scunet-color`, que no se puede descargar porque su export reparte
   los pesos en dos archivos). Esas etapas se omiten y el trabajo sale **degradado**,
   con la nota que lo dice. El escalado siguió siendo con el modelo.
2. **El modelo no se pudo descargar** (sin red, o la URL del manifiesto ya no
   existe). El registro tiene una línea `models.ensured` con el motivo exacto, y la
   nota del item dice `motor de respaldo 'clasico-…'`. Abre **Modelos** para
   descargarlo a mano cuando vuelva la conexión.
3. **El motor no puede cargar ONNX Runtime**, así que no hay modelo posible.
   `GET /v1/capabilities` dice cuál es el motor; si empieza por `clasico-`, mira la
   sección del runtime más arriba (compilaste sin `--features onnx`, o falta la
   biblioteca). En este caso la aplicación **no descarga modelos**: no servirían.

**El puerto está ocupado.** No debería pasar: se pide un puerto libre al sistema
(`--port 0`). Si ocurre, otro proceso está escribiendo en el mismo *portfile*.

### "Reiniciando" en bucle, y se rinde tras 5 intentos

El sidecar arranca y muere repetidamente. El backoff es exponencial (1 s → 30 s)
precisamente para no llenar el log. Mira el log del sidecar: un arranque que
muere al instante casi siempre es una biblioteca ausente o un modelo con formato
incorrecto.

### "Versión de protocolo incompatible"

La aplicación y el sidecar son de versiones distintas. Recompila el sidecar:

```bash
cargo build --release -p su-cli
```

---

## Errores al procesar imágenes

Cada error lleva un código. Esto es lo que significa cada uno y qué hacer.

### `SU-E001` — No se encontró ninguna imagen

Los archivos no son imágenes ni archivos ZIP/CBZ válidos, o la carpeta estaba
vacía. Comprueba la extensión: se aceptan PNG, JPG, WEBP, BMP, TIFF, AVIF, ZIP y
CBZ.

### `SU-E100` / `SU-E102` — No se pudo decodificar / archivo corrupto

El archivo existe pero su contenido no es una imagen válida. Prueba a abrirlo en
otro programa. Si se abre bien y SuperUpscaly falla, adjunta el archivo al
informe de error.

### `SU-E101` — Formato no soportado

Convierte la imagen a PNG, JPG, WEBP, BMP o TIFF.

### `SU-E110` — Falta el modelo

El pipeline necesita un modelo que no está en el caché local. Al pulsar «Upscaly»
la aplicación **descarga antes el modelo de la etapa de escalado**, así que este
error solo aparece cuando esa descarga no se pudo completar (sin red, o el modelo
no es descargable). Desde ese momento la imagen **no se pierde**: el trabajo sigue
con interpolación y queda marcado como degradado, con el motivo en sus notas.

Solo se ve tal cual si el trabajo se lanza desde `su-cli` con `--data-dir`
apuntando a un directorio sin modelos y sin red.

### `SU-E111` — El hash del modelo no coincide

El archivo descargado está corrupto o es otro. Abre **Modelos** y pulsa
**Volver a descargar**: la aplicación borra el archivo y lo trae de nuevo. Si lo
haces a mano, borra el modelo del directorio de modelos y repite la descarga.

### `SU-E112` — No se pudo descargar el modelo

La descarga falló y el modelo sigue sin estar disponible. El aviso incluye la URL
que falló y el motivo, porque no todos los fallos se arreglan igual:

| Motivo | Qué mirar |
| --- | --- |
| `dns` / `connect` / `tls` | Sin red, o un proxy interceptando. Revisa la conexión y, si usas proxy, configúralo en el sistema: la descarga respeta el proxy del sistema operativo. |
| `http 404` / `403` | La URL del manifiesto ya no existe, o el repositorio pasó a requerir sesión. Es un problema del manifiesto: abre un informe con la URL que aparece en el aviso. |
| `timeout` | La conexión se estancó. La descarga se reintenta sola una vez por espejo; si vuelve a pasar, prueba más tarde. |
| `hash` | El archivo llegó entero pero no es el esperado. Se borra solo y **no** se deja a medias: vuelve a intentarlo y, si insiste, el manifiesto apunta a un archivo equivocado. |
| `disk` | Sin espacio, o el directorio de modelos no es escribible. El aviso indica la ruta exacta. |

La descarga es reanudable: lo que ya se había bajado queda en un `.part` y el
siguiente intento continúa desde ahí con `Range`. **No borres los `.part`** si el
fallo fue de red — son el trabajo ya hecho. Sí conviene borrarlos si el motivo fue
`hash`, porque el fragmento guardado es el que no cuadra.

Un modelo que no se puede descargar desde la aplicación (el manifiesto no declara
hash o fuente) lo dice en **Modelos** con «Sin fuente declarada». Esos se colocan
a mano en el directorio de modelos; el botón **Abrir carpeta** lleva hasta él.

### `SU-E120` — Aceleración por hardware no disponible

El execution provider elegido no arranca y se está usando uno más lento. Para ver
cuál está disponible y **por qué no lo están los demás**:

```bash
cargo run -p su-cli -- capabilities
```

Cada proveedor no disponible lleva su motivo: falta la biblioteca del proveedor,
se requiere GPU NVIDIA, no se detectó ninguna GPU…

### `SU-E130` — Se agotó la memoria de la GPU

El sistema ya reintentó con tiles más pequeños. Si aun así aparece:

1. Baja el tile manualmente en **Avanzado → Tamaño de tile** (prueba 256).
2. Activa **Liberar modelo entre imágenes**.
3. Cierra otras aplicaciones que usen la GPU.
4. Como último recurso, pon **Dispositivo → CPU**.

Este error **nunca** produce una imagen corrupta: el reintento es automático y el
trabajo se marca como `degradado` en el resumen.

### `SU-E131` — Se perdió el dispositivo gráfico

El driver se cayó o el equipo entró en suspensión. Guarda el trabajo y reinicia la
aplicación. El lote se puede reanudar desde donde quedó.

### `SU-E141` — El resultado no superó la validación

El modelo devolvió un buffer uniforme, con valores no finitos o con el rango
dinámico colapsado. **El archivo no se escribió**: es preferible un hueco en la
salida que un PNG negro que parezca válido.

Suele indicar un modelo mal convertido a ONNX. Prueba con otro modelo del mismo
tipo.

### `SU-E142` — El resultado sería demasiado grande

A esa escala, la imagen resultante superaría el límite de megapíxeles configurado
(`maxOutputMp`). El trabajo se rechaza **antes de empezar**, así que no se gasta ni
un segundo de GPU.

Baja el factor de escala (un 8x sobre una imagen ya grande rara vez aporta más que
un 4x) o sube el límite en los ajustes avanzados si tienes RAM de sobra. El límite
existe porque 8x sobre 50 MP son 3.2 gigapíxeles: eso no cabe y tumba la aplicación.

### `SU-E143` — El resultado no alcanzó la escala prometida

El pipeline terminó con un tamaño distinto al que anuncia su identificador. Es un
**fallo de programación**, no de tu imagen ni de tu equipo: significa que una etapa
de la cadena no se ejecutó y el resultado quedó más pequeño.

No hay nada que puedas hacer desde la interfaz. El mensaje incluye las etapas que
se omitieron y por qué, así que adjúntalo a un informe de error en el repositorio.

### `SU-E150` — No se pudo escribir la salida

Permisos o espacio en disco. El mensaje incluye la ruta concreta.

### `SU-E161` — No se pudo obtener la ruta del archivo

Ocurre al arrastrar archivos en **Wayland**: el portal XDG no siempre expone la
ruta real. Usa el botón "Seleccionar imagen(es)", que abre el diálogo nativo y
siempre la obtiene.

En macOS con la aplicación en sandbox puede pasar lo mismo; la solución es la
misma.

---

## La comparación antes y después

### «No se puede previsualizar el original»

El formato no se puede dibujar en la ventana. Pasa con **TIFF** y **BMP** (y con un
AVIF que el Chromium de turno no soporte): son formatos que el motor sí decodifica,
por eso el escalado funciona, pero la ventana no los dibuja.

El resultado sí se ve en la vista, y el archivo está guardado. Si quieres el
original al lado, ábrelo con el visor del sistema.

### «No se puede previsualizar el resultado: es demasiado grande»

Puede pasar al comparar una imagen muy grande (un 4x de una foto de muchos
megapíxeles son cientos de megapíxeles que hay que decodificar en memoria). **El
archivo está escrito y es válido**: se abre desde la carpeta de salida o desde
cualquier visor.

### El botón **⇔** no aparece en la fila

Solo aparece cuando la imagen tiene resultado: es decir, cuando terminó bien o
salió marcada como degradada. En una imagen que falló no hay nada que comparar, y
el motivo está en su código de error (`SU-E…`) y en el resumen final.

---

## Errores al compilar el sidecar

### `error: could not exec the linker` o `dlltool: program not found`

Falta `binutils`. Solo afecta al target `x86_64-pc-windows-gnu`, que necesita
`dlltool` para enlazar con `windows-sys`. **En Linux y macOS no aplica.**

```bash
# Debian/Ubuntu
sudo apt install binutils-mingw-w64-x86-64
```

El proyecto evita a propósito dependencias que arrastren `windows-sys`
(`num_cpus`, `sysinfo` y `tempfile` se sustituyeron por la biblioteca estándar)
porque el target GNU no trae el enlazador necesario.

### `failed to run custom build command for 'libsqlite3-sys'`

`rusqlite` con `bundled` compila SQLite desde C. Hace falta un compilador de C:

```bash
# Debian/Ubuntu
sudo apt install build-essential
```

Alternativa sin compilador de C: usar el SQLite del sistema en lugar del
incluido, quitando la feature `bundled` en `services/inference/Cargo.toml` y
teniendo instalado `libsqlite3-dev`.

### Errores en `image` o `axum`

Son errores de compilación con línea y columna. Pásalos tal cual: son localizados
y de un solo sentido.

### `Access denied` al compilar

Ocurre si el proyecto está en una unidad de red o en un sistema de archivos donde
el borrado no funciona. Cargo necesita borrar sus propios artefactos. Solución:
compilar en una ruta local.

---

## Problemas de plataforma

### Linux

**AppImage no arranca.** `chrome-sandbox` necesita permisos SUID:

```bash
sudo chown root:root squashfs-root/chrome-sandbox
sudo chmod 4755 squashfs-root/chrome-sandbox
```

El paquete DEB lo hace solo desde su `postinst`.

**Arrastrar y soltar no hace nada.** En Wayland, mira el apartado de `SU-E161`.

### Windows

**Rutas muy largas.** Por encima de 240 caracteres, Windows necesita el prefijo
`\\?\`. El sidecar lo añade al invocar el proceso; la interfaz muestra la ruta
normal.

### macOS

**"La aplicación está dañada".** Ocurre con binarios no notarizados descargados
por navegador. Es un problema de Gatekeeper, no del binario.

---

## Rendimiento

### Va más lento de lo esperado

1. Comprueba el EP activo: `cargo run -p su-cli -- capabilities`.
2. Si dice `CPU` con una GPU disponible, falta la biblioteca del proveedor.
3. Mira si el resumen final marca imágenes como **degradadas**: significa que se
   redujo el tile por falta de memoria.
4. Revisa el tamaño de tile. Un tile demasiado pequeño multiplica el número de
   pasadas y el coste de las costuras.

### La primera ejecución de un modelo es lenta

Es normal: TensorRT compila su motor la primera vez. Puede tardar minutos. A
partir de ahí se reutiliza, salvo que cambies el driver o la versión de ONNX
Runtime, en cuyo caso se invalida y se recompila.

---

## Cómo reportar un problema

Adjunta:

1. El ZIP de **Exportar diagnóstico** (ya enmascara tus rutas).
2. El código de error, si lo hay.
3. La salida de `cargo run -p su-cli -- capabilities`.
4. Qué esperabas que pasara y qué pasó.

Los códigos de error existen precisamente para que un informe sea útil sin
necesidad de describir la escena.
