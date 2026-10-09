// array-of-structs updates (reference for structs.nyra)
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct { int64_t x, y, vx, vy; } particle;

int main(void) {
    int64_t seed = 12345;
    int64_t n = 100000;
    particle *ps = malloc((size_t)n * sizeof *ps);
    for (int64_t i = 0; i < n; i++) {
        seed = (seed * 1103515245 + 12345) % 2147483648;
        int64_t x = (seed / 65536) % 1000;
        seed = (seed * 1103515245 + 12345) % 2147483648;
        int64_t y = (seed / 65536) % 1000;
        seed = (seed * 1103515245 + 12345) % 2147483648;
        ps[i] = (particle){ x, y, (seed / 65536) % 7 - 3, (seed / 1024) % 5 - 2 };
    }
    for (int step = 0; step < 200; step++)
        for (int64_t i = 0; i < n; i++) {
            particle *p = &ps[i];
            p->x += p->vx;
            p->y += p->vy;
            if (p->x < 0 || p->x >= 1000) p->vx = -p->vx;
            if (p->y < 0 || p->y >= 1000) p->vy = -p->vy;
        }
    int64_t sx = 0, sy = 0;
    for (int64_t i = 0; i < n; i++) { sx += ps[i].x; sy += ps[i].y; }
    printf("%lld %lld\n", (long long)sx, (long long)sy);
    free(ps);
    return 0;
}
