FROM ubuntu:24.04

ARG TRAILMARK_VERSION=latest
ARG FALLBACK_VERSION=0.5.5

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && if [ "$TRAILMARK_VERSION" = "latest" ]; then \
         DEB_URL=$(curl -fsSL https://api.github.com/repos/EdelmarSchneider/trailmark/releases/latest 2>/dev/null | grep -oE 'https://[^"]+_amd64\.deb' | head -n 1 || true); \
       fi \
    && if [ -z "${DEB_URL:-}" ]; then \
         VER="${TRAILMARK_VERSION}"; \
         if [ "$VER" = "latest" ]; then VER="${FALLBACK_VERSION}"; fi; \
         DEB_URL="https://github.com/EdelmarSchneider/trailmark/releases/download/v${VER}/TrailMark_${VER}_amd64.deb"; \
       fi \
    && curl -fsSL "$DEB_URL" -o /tmp/trailmark.deb \
    && apt-get install -y --no-install-recommends /tmp/trailmark.deb \
    && rm -rf /tmp/trailmark.deb /var/lib/apt/lists/*

WORKDIR /workspace

ENTRYPOINT ["trailmark", "mcp"]

