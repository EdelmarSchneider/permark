FROM ubuntu:24.04

ARG PERMARK_VERSION=latest
ARG FALLBACK_VERSION=0.6.1

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && if [ "$PERMARK_VERSION" = "latest" ]; then \
         DEB_URL=$(curl -fsSL https://api.github.com/repos/EdelmarSchneider/permark/releases/latest 2>/dev/null | grep -oE 'https://[^"]+_amd64\.deb' | head -n 1 || true); \
       fi \
    && if [ -z "${DEB_URL:-}" ]; then \
         VER="${PERMARK_VERSION}"; \
         if [ "$VER" = "latest" ]; then VER="${FALLBACK_VERSION}"; fi; \
         DEB_URL="https://github.com/EdelmarSchneider/permark/releases/download/v${VER}/PerMark_${VER}_amd64.deb"; \
       fi \
    && curl -fsSL "$DEB_URL" -o /tmp/permark.deb \
    && apt-get install -y --no-install-recommends /tmp/permark.deb \
    && rm -rf /tmp/permark.deb /var/lib/apt/lists/*

WORKDIR /workspace

ENTRYPOINT ["permark", "mcp"]

