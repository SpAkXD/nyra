// sieve of Eratosthenes to 10,000,000, ten rounds (reference for sieve.nyra)
package main

import "fmt"

func countPrimes(n int64) int64 {
	sieve := make([]bool, n+1)
	for i := range sieve {
		sieve[i] = true
	}
	sieve[0], sieve[1] = false, false
	for i := int64(2); i*i <= n; i++ {
		if sieve[i] {
			for j := i * i; j <= n; j += i {
				sieve[j] = false
			}
		}
	}
	var count int64
	for k := int64(0); k <= n; k++ {
		if sieve[k] {
			count++
		}
	}
	return count
}

func main() {
	var total int64
	for round := int64(0); round < 10; round++ {
		total += countPrimes(10000000 - round)
	}
	fmt.Println(total)
}
