// recursion: fib(n) for n = 0 to 35, summed (reference for fib.nyra)
package main

import "fmt"

func fib(n int64) int64 {
	if n < 2 {
		return n
	}
	return fib(n-1) + fib(n-2)
}

func main() {
	var total int64
	for n := int64(0); n < 36; n++ {
		total += fib(n)
	}
	fmt.Println(total)
}
