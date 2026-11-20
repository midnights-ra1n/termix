# syntax=docker/dockerfile:1
# Termix in a browser: the Linux build runs in a virtual desktop served by noVNC (http://<host>:5800).
FROM rust:1-bookworm AS build
ENV COREPACK_ENABLE_DOWNLOAD_PROMPT=0
RUN apt-get update && apt-get install -y --no-install-recommends \
      libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev libdbus-1-dev \
    && curl -fsSL https://deb.nodesource.com/setup_24.x | bash - && apt-get install -y nodejs \
    && rm -rf /var/lib/apt/lists/* && corepack enable
WORKDIR /src
COPY . .
RUN pnpm install --frozen-lockfile && pnpm tauri build --no-bundle

FROM jlesage/baseimage-gui:debian-12-v4
RUN add-pkg libwebkit2gtk-4.1-0 libayatana-appindicator3-1 librsvg2-common libdbus-1-3 fonts-dejavu-core
COPY --from=build /src/src-tauri/target/release/termix /usr/bin/termix
RUN set-cont-env APP_NAME "Termix"
COPY <<'SH' /startapp.sh
#!/bin/sh
# Settings, hosts and known_hosts live in the /config volume.
export HOME=/config
# No GPU in the virtual display: software rendering for WebKitGTK.
export WEBKIT_DISABLE_DMABUF_RENDERER=1 WEBKIT_DISABLE_COMPOSITING_MODE=1
exec /usr/bin/termix
SH
RUN chmod +x /startapp.sh
