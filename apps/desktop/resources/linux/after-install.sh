#!/bin/sh
# Se ejecuta despues de instalar el paquete .deb.
#
# Chromium (y por tanto Electron) usa un sandbox que necesita que
# `chrome-sandbox` sea propiedad de root y tenga el bit SUID. Los paquetes de
# Debian no pueden llevar esos permisos dentro del archivo, asi que hay que
# aplicarlos al instalar.
#
# Sin esto, la aplicacion falla al arrancar con:
#   The SUID sandbox helper binary was found, but is not configured correctly.

set -e

SANDBOX=/opt/SuperUpscaly/chrome-sandbox

if [ -e "$SANDBOX" ]; then
  chown root:root "$SANDBOX" || true
  chmod 4755 "$SANDBOX" || true
fi
