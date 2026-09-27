# Guía de contribución

Gracias por el interés. Esta guía recoge las convenciones que hacen que el
proyecto siga siendo mantenible, y sobre todo **las reglas que no son negociables**
porque son las que evitan los fallos que el proyecto existe para eliminar.

---

## Puesta en marcha

```bash
npm install                          # interfaz y proceso principal
npm run dev                          # aplicación en desarrollo

cd services/inference && cargo test  # sidecar (363 tests, sin GPU ni modelos)
npm test                             # TypeScript (92 tests)
```

Antes de abrir un PR:

```bash
npm run typecheck                    # los cuatro paquetes de TypeScript
npm test
cd services/inference && cargo test
```

---

## Arquitectura en una vista

```
apps/desktop/      Electron: ventana, IPC, supervisor del sidecar
apps/renderer/     Interfaz: Next.js + React + Jotai
packages/shared/   Contratos compartidos: tipos, errores, i18n
packages/ui/       Primitivas visuales

services/inference/
  su-core          Dominio, errores, motor de pipelines
  su-tiling        Tiles, VRAM, composición
  su-imageio       E/S de imagen, escritura atómica
  su-models        Manifiesto y verificación
  su-hardware      Detección de hardware y EPs
  su-analyze       Ruido y artefactos de compresión
  su-inference     Runner y abstracción de backend
  su-jobs          Cola, persistencia, eventos
  su-server        API HTTP + WebSocket
  su-cli           Interfaz de línea de comandos
```

**La regla estructural:** el sidecar no depende de Electron y la interfaz no
depende de Rust. La frontera es HTTP y está descrita en
`packages/shared/src/sidecar.ts`. Si un cambio obliga a romper esa separación, es
señal de que el diseño está mal, no de que la frontera sobre.

**Importaciones dentro de `packages/shared`:** la extensión se escribe siempre
(`from './types.ts'`). No es estilo: las pruebas del proceso principal cargan
TypeScript directamente con `node --test`, y Node no completa las extensiones que
faltan, así que sin ellas el paquete no se puede probar en ejecución. Ver ADR-036.

---

## Reglas no negociables

### 1. Ningún fallo es silencioso

Todo error que llega al usuario tiene **código, mensaje y acción sugerida**. Si
añades un camino de error, añade su código a
`packages/shared/src/error-codes.ts` y su traducción a los dos idiomas.

Un `false` o un `Option::None` donde el usuario esperaría una explicación es un
fallo del cambio, no una simplificación.

### 2. Nada se escribe sin validarse

La salida de imagen pasa por `su_imageio::validate_output` antes de tocar el disco,
y se escribe en un temporal que se renombra al final. No hay excepciones: un PNG
negro que parece válido es peor que un hueco en la carpeta de salida.

### 3. Lo que se omite, se dice

Si una etapa del pipeline no se ejecuta por su condición, aparece en
`RunOutcome::skipped` con el motivo. Si un modelo no está verificado por hash, se
reporta como `Unverified`, no como `Installed`. Mentir por omisión es la forma más
fácil de perder la confianza del usuario.

### 4. Los tests cubren la lógica, no los detalles

`su-tiling` y `su-core` dedican más de la mitad de su código a tests. No es
casualidad: son la aritmética de tiles, la normalización de pesos y la evaluación
de condiciones, donde un error no se ve a simple vista pero arruina el resultado.

**Un test que solo comprueba que una función no lanza** no aporta nada. Los tests
útiles aquí son los que verifican propiedades: que los pesos suman exactamente 1 en
los solapes, que el producto de los factores de un pipeline coincide con su escala,
que un pico de VRAM absurdo se descarta.

### 5. Sin dependencias que no se usen

Cada dependencia declarada se compila aunque no se referencie. El workspace evita
a propósito las que arrastran `windows-sys` (necesita `dlltool`, que el target GNU
de Rust no incluye). Antes de añadir una, comprueba si se resuelve en cincuenta
líneas.

---

## Cómo añadir un modelo

Sin tocar código. Ver `docs/04-modelos-y-pipelines.md`. En resumen:

1. Convertir a ONNX y verificar la paridad numérica con la implementación original.
2. Calcular el `sha256`.
3. Añadir la entrada al manifiesto con sus pistas de tiling.

El modelo aparece solo en el selector. Si falta el hash, se marcará como
`Unverified` y se avisará: no se finge que está verificado.

---

## Cómo añadir una etapa al pipeline

1. Añade la variante a `StageOp` en `su-core/src/pipeline.rs`.
2. Impleméntala en `su-inference/src/runner.rs`.
3. Si necesita variables nuevas, añádelas a `EvalVars::KNOWN_PATHS` **y** a
   `lookup`. Una variable que no esté en `KNOWN_PATHS` hace que las condiciones que
   la usen fallen al cargar el pipeline, que es lo que se quiere: mejor un error al
   arrancar que un `false` silencioso en tiempo de ejecución.
4. Añade su clave de traducción a `progress.stage.*`.

---

## Estilo

- **Comentarios: el porqué, no el qué.** El código ya dice qué hace. Lo que hay que
  escribir es por qué se eligió así y qué alternativa se descartó.
- **Español en el código** (nombres de dominio, comentarios, mensajes), inglés en
  los identificadores técnicos que ya son estándar.
- **Nada de emojis** en el código ni en la documentación técnica.
- Los documentos de `docs/` son la especificación viva. Si el código se desvía, se
  actualiza el documento **y** se añade una ADR explicando por qué. No se deja
  mintiendo.

---

## Antes de abrir un PR

- [ ] `npm run typecheck` pasa en los cuatro paquetes.
- [ ] `cargo test --workspace --all-features` pasa en `services/inference` (363 pruebas; 344 sin la feature `onnx`).
- [ ] Los cambios de comportamiento están reflejados en `docs/`.
- [ ] Si se tomó una decisión de diseño, hay una ADR en
      `docs/03-decisiones-adr.md`.
- [ ] Si se añadió un camino de error, tiene código y traducción en los dos idiomas.
- [ ] No se añadieron dependencias sin justificarlo en la descripción del PR.

---

## Áreas donde hace falta ayuda

- **Backends de aceleración**: la implementación de ONNX Runtime existe pero no se
  ha probado en hardware real. Los informes de `su-cli capabilities` en GPUs
  distintas son valiosos.
- **Calidad**: el harness de benchmark con SSIM y LPIPS contra Upscayl (Fase 5).
- **Análisis previo**: detección de rostros y clasificador foto/ilustración.
- **Empaquetado**: `chrome-sandbox` en AppImage, notarización en macOS.
