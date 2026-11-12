/*
 * Klucz publiczny podpisu manifestu Shrimp Install (RSA-2048, e = 65537).
 *
 * Wygeneruj parę kluczy i nagłówek poleceniem z repozytorium Shrimp:
 *   shrimp-install/tools/make-key.sh klucz-prywatny.pem > shrimp-install/rufus/src/shrimp/shrimp_key.h
 * Klucza prywatnego nie dodawaj do repozytorium.
 *
 * Bez klucza (SHRIMP_PUBKEY_SET 0) podpis nie jest sprawdzany: zaufanie do manifestu
 * daje wtedy tylko HTTPS. Suma SHA-256 obrazu jest sprawdzana zawsze.
 */
#pragma once
#define SHRIMP_PUBKEY_SET 0
static const uint8_t shrimp_pubkey_modulus[257] = { 0 };
