// array-of-structs updates: 100,000 particles bouncing in a box for 200 steps (reference for structs.nyra)
package main

import "fmt"

type particle struct{ x, y, vx, vy int64 }

func main() {
	var seed int64 = 12345
	next := func() int64 {
		seed = (seed*1103515245 + 12345) % 2147483648
		return seed
	}
	const n = 100000
	ps := make([]particle, n)
	for i := range ps {
		x := (next() / 65536) % 1000
		y := (next() / 65536) % 1000
		s := next()
		ps[i] = particle{x, y, (s/65536)%7 - 3, (s/1024)%5 - 2}
	}
	for step := 0; step < 200; step++ {
		for i := range ps {
			p := &ps[i]
			p.x += p.vx
			p.y += p.vy
			if p.x < 0 || p.x >= 1000 {
				p.vx = -p.vx
			}
			if p.y < 0 || p.y >= 1000 {
				p.vy = -p.vy
			}
		}
	}
	var sx, sy int64
	for _, p := range ps {
		sx += p.x
		sy += p.y
	}
	fmt.Printf("%d %d\n", sx, sy)
}
