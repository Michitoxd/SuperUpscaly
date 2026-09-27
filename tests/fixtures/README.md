# Conjunto de referencia

Aquí van las imágenes con las que se mide el rendimiento y la calidad. **No se
versionan**: son fotos y dibujos reales, y meter material de terceros en el
repositorio traería problemas de licencia además de peso.

## Qué debe contener

El plan fija el conjunto en **20 imágenes: 10 fotografías y 10 ilustraciones**, en
dos resoluciones (1024×1024 y 2048×2048). Ese equilibrio no es decorativo: un
conjunto solo de fotos mediría bien el modelo de fotos y mal el de dibujo, y las
conclusiones del informe saldrían sesgadas.

### Fotografías (10)

Busca variedad en lo que rompe a los modelos:

- Retratos con caras claras y de perfil (la restauración facial solo se activa
  sobre caras detectadas).
- Paisajes con detalle fino: follaje, texturas de piedra, agua.
- Fotos con ruido real de ISO alto.
- Alguna con JPEG agresivo, para comprobar que el enfoque final se inhibe.

### Ilustraciones (10)

- Anime con líneas limpias y zonas planas de color.
- Manga en blanco y negro con tramado.
- Ilustración con degradados suaves.
- Alguna con compresión JPEG, que es lo habitual en material descargado.

### Formatos

PNG para las imágenes de referencia. Un JPEG de entrada introduce artefactos que
contaminan la comparación de calidad.

## Cómo se usan

```bash
# Solo SuperUpscaly
node scripts/benchmark.mjs --fixtures tests/fixtures

# Comparación con Upscayl
node scripts/benchmark.mjs --fixtures tests/fixtures --upscayl-bin ~/upscayl-bin
```

El informe se escribe en `docs/benchmarks/AAAA-MM-DD.md`.

## Aviso sobre el sesgo

Las conclusiones de un benchmark solo valen para lo que se ha medido. Un conjunto
de 20 imágenes no demuestra que un modelo sea mejor en general: demuestra que lo es
**en esas 20**. Por eso el informe publica siempre la tabla por imagen y no solo el
agregado, para que se pueda ver si un resultado medio bueno esconde casos malos.
