/** @type {import('next').NextConfig} */
const nextConfig = {
  // El renderer se sirve desde el esquema `app://` del proceso principal, no
  // desde un servidor Node. Un export estatico elimina ese proceso por completo.
  output: 'export',

  reactStrictMode: true,

  // Sin servidor de imagenes: `next/image` se usa en modo no optimizado.
  images: { unoptimized: true },

  // Los paquetes del monorepo se consumen como TypeScript sin compilar.
  transpilePackages: ['@superupscaly/shared', '@superupscaly/ui'],

  // Origenes adicionales admitidos en desarrollo.
  //
  // El servidor de desarrollo de Next solo negocia el socket de recarga del
  // cliente (el que hace falta para que la pagina hidrate) cuando el `Host` de la
  // peticion es uno de los suyos. `127.0.0.1` no lo es, y sin ese socket la
  // interfaz se pinta y no responde a nada (ver `scripts/dev.mjs`, que ya carga
  // la ventana por `localhost`). Esta lista existe para que abrir la IP a mano
  // —o un SU_DEV_URL con IP— tampoco rompa la aplicacion en silencio.
  allowedDevOrigins: ['127.0.0.1', 'localhost'],

  // Rutas relativas para que el export funcione bajo cualquier origen.
  trailingSlash: false,

  productionBrowserSourceMaps: false,
}

export default nextConfig
