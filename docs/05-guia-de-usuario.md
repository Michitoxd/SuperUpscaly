# 05 · Guía de usuario

> Cómo usar SuperUpscaly y, sobre todo, **cuándo conviene cambiar los ajustes**.
> Los valores por defecto están elegidos para que no haga falta tocar nada.

---

## Qué hace

Escala imágenes con redes neuronales, en tu equipo, sin enviar nada a ningún
sitio. Elige una o varias imágenes, marca si son fotografías o dibujos, y pulsa
**Upscaly**.

---

## El flujo normal

1. **Arrastra las imágenes** a la ventana, o pulsa "Seleccionar imagen(es)".
   También puedes arrastrar una carpeta entera: se expande sola. Y también un
   archivo **ZIP** o **CBZ**.
2. **Elige el modo**: Fotos o Dibujo/Anime.
3. **Elige la escala**: 2x, 4x u 8x.
4. **Pulsa Upscaly.**
5. Cuando el trabajo termina, en la fila de cada imagen aparece un botón **⇔** para
   comparar el antes y el después (ver abajo).

El resultado se guarda en `~/Pictures/Upscaled` (o donde hayas elegido), con el
sufijo `_upscaled`. El nombre original se conserva.

### Archivos ZIP y CBZ

Un **CBZ** es un ZIP con imágenes dentro: el formato de los cómics digitales. Si
arrastras uno, se extraen sus imágenes y entran en el lote **en el orden en que
están guardadas**, que es el orden de las páginas. Ese orden no se reordena por
nombre, así que un cómic cuyas páginas no lleven número sale bien.

**Cada página es una fila.** No verás una sola fila con el nombre del `.cbz`:
verás una por página, cada una con su tamaño, su progreso, su resultado y su botón
de comparar. Son las páginas las que se procesan, así que son ellas las que se
cuentan, y el resumen final cuadra con lo que hay en la lista.

Se ignoran las entradas que no son imágenes (el `ComicInfo.xml` típico), las
carpetas, los archivos ocultos y la basura que deja macOS al comprimir
(`__MACOSX`). Si dos imágenes comprimidas se llaman igual pero están en carpetas
distintas, la segunda se guarda como `nombre (2).ext` en vez de pisar la primera.

**Si el archivo está dañado, no se abre a medias.** Cada entrada lleva un CRC, y
si no cuadra se te avisa **al añadirlo**, con el nombre del archivo y el motivo, y
su fila se queda en la lista en el sitio que le tocaba. Al pulsar Upscaly el
trabajo se rechaza con el mismo motivo, en lugar de procesar media docena de
páginas y dejar el resto sin explicación. Lo mismo con un ZIP cifrado con
contraseña, uno que use compresión que no se soporta o uno sin ninguna imagen
dentro: verás el motivo en lugar de un resultado raro.

---

## Comparar el antes y el después

En la fila de una imagen ya escalada hay un botón **⇔**. Abre una vista con la
imagen original y el resultado **en el mismo sitio**: a la izquierda se ve el
original y a la derecha lo que produjo el motor, separados por una línea que se
mueve.

- **Arrastra en cualquier punto de la imagen** para mover la línea. No hace falta
  acertar en el tirador: pisando la imagen ya se mueve.
- **Con el teclado**: el tirador tiene el foco (tecla `Tab`). Las flechas mueven la
  línea un 2 %, `Mayúsculas` + flechas un 10 %, `Inicio` la lleva a la izquierda del
  todo y `Fin` a la derecha.
- **`Escape`** cierra la vista, igual que el botón «Cerrar».

Las dos mitades están a la misma escala y alineadas píxel con píxel, así que lo que
ves a un lado de la línea corresponde exactamente a lo mismo del otro lado. El
fondo a cuadros muestra **la transparencia**: si el original tiene canal alfa (un
PNG recortado, por ejemplo), ahí se ve dónde acaba de verdad la imagen.

En el pie de la vista están las medidas de cada lado (`128 x 128 px` frente a
`512 x 512 px`), lo que tardó, el botón para abrir la carpeta del resultado y, si
la imagen salió distinta de lo configurado, el motivo (por ejemplo, una etapa que
se omitió).

Dos límites que conviene conocer, porque se dicen en pantalla en lugar de dejar un
hueco:

- **Un formato que la ventana no dibuja** (TIFF, BMP) no se puede previsualizar. El
  resultado sí; el original no, y la vista lo explica y deja ver el resultado solo.
- **Una imagen enorme** puede no caber en memoria para mostrarla. El archivo está
  guardado igual: se abre desde la carpeta de salida.

---

## Los dos modos

Solo hay dos, y es deliberado. Un desplegable con quince modelos obliga al usuario
a saber qué es un "RRDBNet de 6 bloques", y elegir mal se nota mucho en el
resultado.

| Modo | Para qué | Qué usa |
|---|---|---|
| **Fotos** | Fotografías, capturas de cámara, imágenes con ruido o grano | Un modelo entrenado con fotografías, más restauración facial si detecta caras |
| **Dibujo / Anime** | Ilustración, anime, manga, arte digital, cómics | Un modelo entrenado con dibujo, que respeta las líneas y las zonas planas |

**Cómo elegir si dudas:** si la imagen tiene zonas de color plano grandes y líneas
limpias, es Dibujo/Anime. Si tiene grano, texturas irregulares y degradados
suaves, es Fotos.

Usar el modo equivocado no rompe nada, pero el resultado se nota: el modelo de
fotos sobre un dibujo añade textura donde debería haber color plano, y el de anime
sobre una foto deja la piel con aspecto de plástico.

---

## Las escalas

| Escala | Cuándo usarla | Qué hace por dentro |
|---|---|---|
| **2x** | Cuando solo necesitas el doble y el archivo original ya es grande | Escala a 4x y reduce a la mitad con Lanczos |
| **4x** | El caso normal | Una pasada del modelo a 4x |
| **8x** | Solo si de verdad lo necesitas | Escala a 4x, reduce a la mitad y vuelve a escalar a 4x |

**8x es caro.** Cuatro veces el tiempo de 4x y un archivo de salida enorme: una
imagen de 12 MP pasa a 768 MP, unos 2 GB en PNG. La aplicación avisa antes de
empezar y ofrece 4x si detecta que el resultado va a ser desmesurado.

**Regla práctica:** escala lo mínimo que necesites. Un 4x sobre una imagen de 12 MP
ya da 192 MP, que es más de lo que necesita cualquier uso normal.

---

## Configuración avanzada

Está colapsada porque los valores por defecto son los correctos en la mayoría de
los casos. Esto es lo que hace cada ajuste y cuándo tocarlo.

### Cadena de modelos

Muestra las etapas que se van a ejecutar. **No es informativo: es lo que va a
pasar.** Si una etapa aparece ahí, se ejecuta.

En automático, la aplicación decide según el modo y el análisis de la imagen. Por
ejemplo, si detecta ruido, añade un paso de reducción antes de escalar; si detecta
compresión JPEG fuerte, quita el enfoque final, porque solo amplificaría los
artefactos.

### Tamaño de tile

**Déjalo en Automático.** Se calcula a partir de la memoria libre de tu GPU y de
lo que consume el modelo, que es información que el usuario no tiene.

Si ves errores de memoria (`SU-E130`), bájalo a 256. Si tienes una GPU con mucha
memoria y quieres el máximo rendimiento, súbelo: menos tiles significa menos
costuras y menos pasadas.

### Dispositivo

Automático usa la mejor GPU disponible y cae a CPU si no hay ninguna. Forzar
**CPU** es útil para comprobar si un fallo viene del driver gráfico. Es mucho más
lento.

### Restauración facial

Solo disponible en modo Fotos. Se aplica **únicamente sobre las caras detectadas**,
no sobre la imagen entera: cada cara se recorta, se restaura y se pega de vuelta con
una transición suave, así que el fondo no se toca y no se ve un parche rectangular.
Los tres niveles de intensidad son 0.6 (Suave), 0.85 (Automática) y 1.0 (Alta).

Útil para fotos antiguas o de baja resolución. En caras que ya se ven bien puede
suavizar de más; en ese caso, usa la intensidad Suave o desactívala.

> **Hoy esta etapa se omite, y lo dice.** El análisis de la imagen todavía no
detecta rostros, así que no hay caras que recortar: en las notas de cada imagen
aparece el motivo «no se detectaron caras». Está implementada y probada la parte
del motor (recorte, máscara y pegado con la intensidad que elijas); lo que falta es
el detector. Cuando exista, bastará con tener el modelo instalado: no hay ningún
ajuste nuevo que activar.

Una cara que ocupe muy poco en la imagen se deja como estaba y la nota dice cuántas
quedaron fuera: por debajo de un recorte de 96 px el modelo inventa rasgos en lugar
de restaurarlos.

### Reducción de ruido

Se ejecuta **antes** de escalar, no después. El orden importa: si se aplicara
después, el modelo ya habría amplificado el ruido y eliminarlo se llevaría por
delante el detalle que acababa de sintetizar.

En Automático se activa solo si la imagen está degradada, y **la cantidad de
reducción la decide el ruido medido**: entre un 50 % en una imagen con ruido
moderado, para no lavar el detalle, y el 100 % en una muy degradada. Un valor fijo
obligaba a elegir el peor caso para todas.

### Enfoque final

Realza el detalle al terminar. Se inhibe automáticamente en imágenes con
artefactos de compresión, donde solo amplificaría los bordes de bloque.

### Imágenes simultáneas

**Déjalo en 1.** No es una limitación: es el valor óptimo medido. Dos sesiones
concurrentes en la misma GPU no aceleran nada (el dispositivo ya está saturado) y
duplican el uso de memoria.

### Liberar modelo entre imágenes

Reduce el pico de memoria a costa de recargar el modelo en cada imagen. Solo tiene
sentido en GPUs con muy poca memoria y lotes pequeños.

---

## Procesamiento por lotes

Se pueden seleccionar cientos de imágenes. La cola muestra el progreso de cada una
y el global.

- **Pausar** detiene el trabajo al terminar la imagen en curso, nunca a mitad de
  una. Por eso reanudar es seguro.
- **Cancelar** hace lo mismo y deja el lote reanudable.
- Si cierras la aplicación a mitad de un lote, al volver a abrirla aparece **en
  pausa** y puedes continuarlo: las imágenes ya procesadas no se repiten.

Al terminar, el resumen indica cuántas se completaron, cuántas fallaron y cuántas
salieron **degradadas**.

### Qué significa "degradada"

La imagen se procesó, pero con una configuración más conservadora de lo previsto:
se redujo el tamaño de tile, o se cayó a CPU, porque no había memoria suficiente.

**El resultado es válido.** Degradada no es un error. Si aparecen muchas, baja el
tamaño de tile para que no haga falta degradar.

---

## El estado del motor

En la barra lateral, siempre visible:

| Estado | Significa |
|---|---|
| **Listo** | Todo correcto |
| **Arrancando** | El motor está cargando |
| **Reiniciando** | Se cayó y se está recuperando |
| **Con error** | No arranca; el motivo aparece debajo |
| **No disponible** | No se encontró el ejecutable |

Si el botón Upscaly no hace nada, mira aquí primero.

---

## Los modelos

Los modelos **no vienen dentro de la aplicación**: son archivos grandes (el mayor
pasa de 300 MB) y cada uno tiene su propia licencia. Se descargan cuando los
necesitas.

### Lo primero que pasa al pulsar «Upscaly»

Si el modelo que necesita el modo y la escala que elegiste **no está descargado, la
aplicación lo descarga antes de empezar**, y la barra de progreso lo dice
(«Preparando: descargando el modelo») con el porcentaje. Son unos 18 MB el de
dibujo y anime, y 33 MB el de fotos: la primera imagen de cada modo tarda un poco
más que las siguientes, y solo una vez.

No hace falta hacer nada a mano. Si no hay conexión, o el modelo no tiene fuente
declarada, el trabajo **se hace igual** con interpolación, queda marcado como
degradado y sus notas dicen con qué motor se hizo. Cuando vuelva la conexión, el
siguiente trabajo lo descargará.

El botón **Modelos** de la barra lateral abre la lista completa del catálogo, con
un número que indica cuántos faltan. Cada fila dice su estado:

| Estado | Significa |
|---|---|
| **Instalado** | El archivo está y su `sha256` coincide |
| **Falta** | No está en el directorio de modelos |
| **Dañado** | El archivo está, pero su `sha256` no coincide: hay que reemplazarlo |
| **Sin fuente declarada** | El catálogo no conoce una URL verificable para ese modelo; se instala a mano |

### Descargar

Pulsa **Descargar** en la fila. Antes de empezar verás la **licencia** del modelo:
léela, porque algunas prohíben el uso comercial y eso te afecta a ti, no a
SuperUpscaly.

Es el mismo botón que la aplicación usa sola para el modelo de escalado; aquí
puedes además adelantarte y bajar los de restauración (rostros, reducción de
ruido), que son opcionales.

La descarga muestra el progreso y se puede **cancelar** en cualquier momento. Si la
cancelas o se corta la conexión, lo que ya se había bajado **no se pierde**: queda
en un archivo `.part` y el siguiente intento continúa desde ahí. Al terminar, el
archivo se verifica contra su `sha256` y solo entonces se renombra a su nombre
definitivo. Un archivo a medias nunca aparece como instalado.

### Instalar a mano

Si prefieres no descargar desde la aplicación, o el modelo no tiene fuente
declarada, pulsa **Abrir carpeta** y coloca el archivo ahí con el nombre exacto que
indica la lista. Al volver a la aplicación aparecerá como instalado, siempre que su
`sha256` coincida con el del catálogo.

### Por qué importa la licencia

La aplicación es MIT, pero **los modelos no lo son**. En el catálogo conviven
licencias BSD, Apache y Creative Commons, y algunas de estas últimas son
**no comerciales**. Si vas a usar el resultado en un trabajo remunerado, comprueba
la licencia del modelo que elijas: la lista la muestra antes de descargar y
`docs/04-modelos-y-pipelines.md` tiene la tabla completa.

---

## Formato de salida

PNG sin pérdida por defecto, que es lo correcto para un resultado que querrás
seguir editando. JPG y WEBP ocupan mucho menos; en WEBP la salida es sin pérdida.

Si la imagen original tenía transparencia, se conserva, y **su contorno no se
difumina**: la silueta la reconstruye el mismo modelo que el color, con la misma
malla de tiles, en lugar de un interpolador aparte. Es lo que evita el halo
blando alrededor de un PNG recortado, el defecto clásico de otras herramientas.
El coste es una pasada de inferencia más por cada etapa que escala, y solo en las
imágenes que tienen transparencia.

---

## Consejos

**No escales dos veces.** Si ya escalaste una imagen, escalar el resultado otra vez
amplifica los artefactos del primer paso. Parte siempre del original.

**Guarda el original.** El resultado no sustituye al archivo de entrada; se escribe
uno nuevo con sufijo.

**Un 4x bien hecho se ve mejor que un 8x forzado.** Si el resultado de 4x ya es
suficiente, no hace falta más.

**En lotes grandes, prueba con tres o cuatro imágenes primero.** Así ves si el modo
elegido da el resultado que esperas antes de lanzar doscientas.
