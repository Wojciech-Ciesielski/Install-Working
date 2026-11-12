/*
 * Shrimp Install: małe budowanie JSON (UTF-8) i odczyt płaskich obiektów.
 * Copyright © 2026 Shrimp. GPLv3.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "shrimp.h"

static void put(jb* b, const char* s, size_t n)
{
	if (b->len + n + 1 > b->cap) {
		size_t cap = (b->cap ? b->cap : 256);
		while (b->len + n + 1 > cap)
			cap *= 2;
		char* p = realloc(b->s, cap);
		if (p == NULL)
			return;
		b->s = p;
		b->cap = cap;
	}
	memcpy(b->s + b->len, s, n);
	b->len += n;
	b->s[b->len] = 0;
}
#define PUTS(b, s) put(b, s, strlen(s))

static void esc(jb* b, const char* v)
{
	char u[8];
	put(b, "\"", 1);
	for (const unsigned char* p = (const unsigned char*)v; p && *p; p++) {
		switch (*p) {
		case '"': PUTS(b, "\\\""); break;
		case '\\': PUTS(b, "\\\\"); break;
		case '\n': PUTS(b, "\\n"); break;
		case '\r': PUTS(b, "\\r"); break;
		case '\t': PUTS(b, "\\t"); break;
		default:
			if (*p < 0x20) {
				snprintf(u, sizeof(u), "\\u%04x", *p);
				PUTS(b, u);
			} else {
				put(b, (const char*)p, 1);
			}
		}
	}
	put(b, "\"", 1);
}

static void sep(jb* b, const char* key)
{
	if (b->depth > 0) {
		if (!b->first[b->depth])
			put(b, ",", 1);
		b->first[b->depth] = FALSE;
	}
	if (key != NULL) {
		esc(b, key);
		put(b, ":", 1);
	}
}

void jb_init(jb* b) { memset(b, 0, sizeof(*b)); put(b, "", 0); }
void jb_text(jb* b, const char* s) { PUTS(b, s); }
void jb_free(jb* b) { free(b->s); memset(b, 0, sizeof(*b)); }

static void jb_open(jb* b, const char* key, const char* c)
{
	sep(b, key);
	PUTS(b, c);
	if (b->depth < 31)
		b->depth++;
	b->first[b->depth] = TRUE;
}
void jb_obj(jb* b, const char* key) { jb_open(b, key, "{"); }
void jb_arr(jb* b, const char* key) { jb_open(b, key, "["); }
void jb_end_obj(jb* b) { b->depth--; PUTS(b, "}"); }
void jb_end_arr(jb* b) { b->depth--; PUTS(b, "]"); }

void jb_str(jb* b, const char* key, const char* v)
{
	sep(b, key);
	if (v == NULL)
		PUTS(b, "null");
	else
		esc(b, v);
}

void jb_wstr(jb* b, const char* key, const wchar_t* v)
{
	if (v == NULL) {
		jb_null(b, key);
		return;
	}
	int n = WideCharToMultiByte(CP_UTF8, 0, v, -1, NULL, 0, NULL, NULL);
	char* s = malloc(n > 0 ? n : 1);
	if (s == NULL)
		return;
	s[0] = 0;
	WideCharToMultiByte(CP_UTF8, 0, v, -1, s, n, NULL, NULL);
	jb_str(b, key, s);
	free(s);
}

void jb_num(jb* b, const char* key, double v)
{
	char t[64];
	sep(b, key);
	snprintf(t, sizeof(t), "%.17g", v);
	PUTS(b, t);
}

void jb_u64(jb* b, const char* key, uint64_t v)
{
	char t[32];
	sep(b, key);
	snprintf(t, sizeof(t), "%llu", (unsigned long long)v);
	PUTS(b, t);
}

void jb_bool(jb* b, const char* key, BOOL v) { sep(b, key); PUTS(b, v ? "true" : "false"); }
void jb_null(jb* b, const char* key) { sep(b, key); PUTS(b, "null"); }
void jb_raw(jb* b, const char* key, const char* raw) { sep(b, key); PUTS(b, raw); }

/*
 * Wartość klucza z płaskiego obiektu JSON (manifest Shrimp): "klucz": "tekst" albo "klucz": 123.
 * Obsługuje \" \\ \/ w tekstach; \uXXXX nie jest potrzebne w manifeście.
 */
BOOL json_get(const char* json, const char* key, char* out, size_t out_size)
{
	char pat[80];
	const char* p;
	size_t n = 0;

	if (json == NULL || out_size == 0)
		return FALSE;
	snprintf(pat, sizeof(pat), "\"%s\"", key);
	for (p = strstr(json, pat); p != NULL; p = strstr(p + 1, pat)) {
		const char* q = p + strlen(pat);
		while (*q == ' ' || *q == '\t' || *q == '\r' || *q == '\n')
			q++;
		if (*q != ':')
			continue;
		q++;
		while (*q == ' ' || *q == '\t' || *q == '\r' || *q == '\n')
			q++;
		if (*q == '"') {
			for (q++; *q && *q != '"' && n + 1 < out_size; q++) {
				if (*q == '\\' && q[1])
					q++;
				out[n++] = *q;
			}
		} else {
			for (; *q && (strchr("-+.eE", *q) || (*q >= '0' && *q <= '9')) && n + 1 < out_size; q++)
				out[n++] = *q;
		}
		out[n] = 0;
		return n > 0;
	}
	return FALSE;
}
