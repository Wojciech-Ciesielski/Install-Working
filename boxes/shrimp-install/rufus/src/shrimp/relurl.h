/*
 * Shrimp Install: adres zasobu wydania w odpowiedzi API GitHub (releases/latest, releases/tags/…).
 * Tylko libc, żeby dało się to sprawdzić testem na Linuksie (tests/relurl_test.c).
 * Copyright © 2026 Shrimp. GPLv3.
 */
#ifndef SHRIMP_RELURL_H
#define SHRIMP_RELURL_H
#include <stdio.h>
#include <string.h>

/* Szuka w JSON obiektu z "name": <name> i zwraca jego "url" (…/releases/assets/<id>).
 * Klucz "url" zasobu stoi przed jego "name", a następny "url" (uploader) dopiero po. */
static int shrimp_release_asset_url(const char* json, const char* name, char* out, size_t out_size)
{
	static const char URL_KEY[] = "\"url\":\"";
	size_t nl = strlen(name);
	const char* p = json;
	while ((p = strstr(p, "\"name\"")) != NULL) {
		const char* v = p + 6;
		p = v;
		while (*v == ' ' || *v == ':')
			v++;
		if (*v != '"')
			continue;
		v++;
		if (strncmp(v, name, nl) != 0 || v[nl] != '"')
			continue;
		/* Ostatnie "url":"…/releases/assets/…" przed tym miejscem. */
		const char* best = NULL;
		const char* q = json;
		while ((q = strstr(q, URL_KEY)) != NULL && q < v) {
			const char* s = q + sizeof(URL_KEY) - 1;
			const char* e = strchr(s, '"');
			if (e != NULL && strstr(s, "/releases/assets/") != NULL && strstr(s, "/releases/assets/") < e)
				best = s;
			q = s;
		}
		if (best == NULL)
			continue;
		const char* e = strchr(best, '"');
		size_t n = (size_t)(e - best);
		if (e == NULL || n + 1 > out_size)
			return 0;
		memcpy(out, best, n);
		out[n] = 0;
		return 1;
	}
	return 0;
}
#endif
