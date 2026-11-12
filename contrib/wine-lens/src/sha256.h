/* SPDX-License-Identifier: MIT
 *
 * SHA-256 and HMAC-SHA-256 (FIPS 180-4, RFC 2104), for the handshake between the camera bridge and its
 * client: both prove that they know the shared secret without sending it. Plain C, no library, so that the
 * native bridge and the Windows-side client compile the same code.
 */
#ifndef A2O_SHA256_H
#define A2O_SHA256_H
#include <stdint.h>
#include <string.h>

struct a2o_sha256 {
    uint32_t state[8];
    uint64_t bytes;
    unsigned char block[64];
};

static void a2o_sha256_block(struct a2o_sha256 *s, const unsigned char *p)
{
    static const uint32_t k[64] = {
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    };
    uint32_t w[64], v[8];
#define A2O_ROR(x, n) (((x) >> (n)) | ((x) << (32 - (n))))
    for (int i = 0; i < 16; i++)
        w[i] = (uint32_t)p[i * 4] << 24 | (uint32_t)p[i * 4 + 1] << 16 | (uint32_t)p[i * 4 + 2] << 8 | p[i * 4 + 3];
    for (int i = 16; i < 64; i++) {
        uint32_t a = A2O_ROR(w[i - 15], 7) ^ A2O_ROR(w[i - 15], 18) ^ (w[i - 15] >> 3);
        uint32_t b = A2O_ROR(w[i - 2], 17) ^ A2O_ROR(w[i - 2], 19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16] + a + w[i - 7] + b;
    }
    memcpy(v, s->state, sizeof v);
    for (int i = 0; i < 64; i++) {
        uint32_t t1 = v[7] + (A2O_ROR(v[4], 6) ^ A2O_ROR(v[4], 11) ^ A2O_ROR(v[4], 25)) + ((v[4] & v[5]) ^ (~v[4] & v[6])) +
                      k[i] + w[i];
        uint32_t t2 = (A2O_ROR(v[0], 2) ^ A2O_ROR(v[0], 13) ^ A2O_ROR(v[0], 22)) +
                      ((v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]));
        v[7] = v[6], v[6] = v[5], v[5] = v[4], v[4] = v[3] + t1, v[3] = v[2], v[2] = v[1], v[1] = v[0], v[0] = t1 + t2;
    }
#undef A2O_ROR
    for (int i = 0; i < 8; i++) s->state[i] += v[i];
}

static void a2o_sha256_start(struct a2o_sha256 *s)
{
    static const uint32_t initial[8] = { 0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                                         0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19 };
    memcpy(s->state, initial, sizeof initial);
    s->bytes = 0;
}

static void a2o_sha256_add(struct a2o_sha256 *s, const void *data, size_t length)
{
    const unsigned char *p = data;
    while (length) {
        size_t at = (size_t)(s->bytes % 64), room = 64 - at, take = length < room ? length : room;
        memcpy(s->block + at, p, take);
        s->bytes += take, p += take, length -= take;
        if (s->bytes % 64 == 0) a2o_sha256_block(s, s->block);
    }
}

static void a2o_sha256_finish(struct a2o_sha256 *s, unsigned char out[32])
{
    uint64_t bits = s->bytes * 8;
    unsigned char pad[72] = { 0x80 }, size[8];
    size_t at = (size_t)(s->bytes % 64);

    for (int i = 0; i < 8; i++) size[i] = (unsigned char)(bits >> (56 - 8 * i));
    a2o_sha256_add(s, pad, (at < 56 ? 56 : 120) - at);
    a2o_sha256_add(s, size, 8);
    for (int i = 0; i < 8; i++)
        for (int j = 0; j < 4; j++) out[i * 4 + j] = (unsigned char)(s->state[i] >> (24 - 8 * j));
}

/* HMAC-SHA-256 of `message` under `key`, as 64 lowercase hex digits and a terminating zero. */
static void a2o_hmac_hex(const char *key, const char *message, char out[65])
{
    unsigned char pad[64] = { 0 }, inner[32], mac[32];
    size_t key_length = strlen(key);
    struct a2o_sha256 s;

    if (key_length > 64) {
        a2o_sha256_start(&s);
        a2o_sha256_add(&s, key, key_length);
        a2o_sha256_finish(&s, pad);
    } else {
        memcpy(pad, key, key_length);
    }
    for (int i = 0; i < 64; i++) pad[i] ^= 0x36;
    a2o_sha256_start(&s);
    a2o_sha256_add(&s, pad, 64);
    a2o_sha256_add(&s, message, strlen(message));
    a2o_sha256_finish(&s, inner);
    for (int i = 0; i < 64; i++) pad[i] ^= 0x36 ^ 0x5c;
    a2o_sha256_start(&s);
    a2o_sha256_add(&s, pad, 64);
    a2o_sha256_add(&s, inner, 32);
    a2o_sha256_finish(&s, mac);
    for (int i = 0; i < 32; i++) {
        out[i * 2] = "0123456789abcdef"[mac[i] >> 4];
        out[i * 2 + 1] = "0123456789abcdef"[mac[i] & 15];
    }
    out[64] = 0;
}

/* Whether two strings are equal, in time that does not depend on where they differ. */
static int a2o_same(const char *a, const char *b)
{
    size_t la = strlen(a), lb = strlen(b);
    unsigned char difference = la != lb;
    for (size_t i = 0; i < la && i < lb; i++) difference |= (unsigned char)(a[i] ^ b[i]);
    return !difference;
}
#endif
