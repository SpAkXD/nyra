// quicksort and the built-in sort of 1,000,000 ints (reference for sort.nyra)
package main

import (
	"fmt"
	"slices"
)

func quicksort(xs []int64, lo, hi int64) {
	if lo >= hi {
		return
	}
	pivot := xs[(lo+hi)/2]
	i, j := lo, hi
	for i <= j {
		for xs[i] < pivot {
			i++
		}
		for xs[j] > pivot {
			j--
		}
		if i <= j {
			xs[i], xs[j] = xs[j], xs[i]
			i++
			j--
		}
	}
	quicksort(xs, lo, j)
	quicksort(xs, i, hi)
}

func main() {
	const n = 1000000
	var seed int64 = 7
	xs := make([]int64, 0, n)
	for k := 0; k < n; k++ {
		seed = (seed*1103515245 + 12345) % 2147483648
		xs = append(xs, (seed/64)%1000000)
	}
	ys := slices.Clone(xs)
	quicksort(xs, 0, int64(len(xs))-1)
	slices.Sort(ys)
	fmt.Println(slices.Equal(xs, ys))
	var check int64
	for _, x := range xs {
		check = (check*31 + x) % 1000000007
	}
	fmt.Println(check)
}
