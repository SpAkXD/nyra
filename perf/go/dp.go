// knapsack (one array) and LCS (a 2D table) (reference for dp.nyra)
package main

import "fmt"

var seed int64 = 42

func next() int64 {
	seed = (seed*1103515245 + 12345) % 2147483648
	return seed
}

func knapsack(w, v []int64, cap int64) int64 {
	best := make([]int64, cap+1)
	for k := range w {
		for c := cap; c >= w[k]; c-- {
			take := best[c-w[k]] + v[k]
			if take > best[c] {
				best[c] = take
			}
		}
	}
	return best[cap]
}

func lcs(a, b []int64) int64 {
	n, m := len(a), len(b)
	dp := make([][]int64, n+1)
	for i := range dp {
		dp[i] = make([]int64, m+1)
	}
	for i := 1; i <= n; i++ {
		for j := 1; j <= m; j++ {
			if a[i-1] == b[j-1] {
				dp[i][j] = dp[i-1][j-1] + 1
			} else if dp[i-1][j] > dp[i][j-1] {
				dp[i][j] = dp[i-1][j]
			} else {
				dp[i][j] = dp[i][j-1]
			}
		}
	}
	return dp[n][m]
}

func main() {
	w := make([]int64, 1000)
	v := make([]int64, 1000)
	for k := 0; k < 1000; k++ {
		w[k] = (next()/65536)%1000 + 1
		v[k] = (next()/65536)%1000 + 1
	}
	fmt.Println(knapsack(w, v, 50000))
	a := make([]int64, 2500)
	b := make([]int64, 2500)
	for k := 0; k < 2500; k++ {
		a[k] = (next() / 65536) % 4
		b[k] = (next() / 65536) % 4
	}
	fmt.Println(lcs(a, b))
}
