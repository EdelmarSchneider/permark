FROM ubuntu:24.04

ARG TRAILMARK_VERSION=0.5.5

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && curl -fsSL "https://github.com/EdelmarSchneider/trailmark/releases/download/v${TRAILMARK_VERSION}/TrailMark_${TRAILMARK_VERSION}_amd64.deb" -o /tmp/trailmark.deb \
    && apt-get install -y --no-install-recommends /tmp/trailmark.deb \
    && rm -rf /tmp/trailmark.deb /var/lib/apt/lists/*

WORKDIR /workspace

ENTRYPOINT ["trailmark", "mcp"]
