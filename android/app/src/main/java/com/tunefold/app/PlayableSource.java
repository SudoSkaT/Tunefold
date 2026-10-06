package com.tunefold.app;

/** Generic remote source contract consumed by the existing AndroidEngine URL decoder. */
final class PlayableSource {
    final String url;
    final String headersJson;

    PlayableSource(String url, String headersJson) {
        this.url = url;
        this.headersJson = headersJson == null ? "[]" : headersJson;
    }
}
