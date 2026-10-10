// string building, split, join, char scans (reference for strings.nyra)
package main

import (
	"fmt"
	"strconv"
	"strings"
)

func main() {
	var sb strings.Builder
	for i := 0; i < 2000000; i++ {
		sb.WriteString(strconv.Itoa(i))
		sb.WriteByte(',')
	}
	s := sb.String()
	fmt.Println(len(s))
	sevens := 0
	for k := 0; k < len(s); k++ {
		if s[k] == '7' {
			sevens++
		}
	}
	fmt.Println(sevens)
	var total int64
	for _, p := range strings.Split(s, ",") {
		if len(p) > 0 {
			n, _ := strconv.ParseInt(p, 10, 64)
			total += n
		}
	}
	fmt.Println(total)
	lines := make([]string, 0, 500000)
	for i := 0; i < 500000; i++ {
		lines = append(lines, fmt.Sprintf("item %d: %d of %d", i, i*3, i%7))
	}
	fmt.Println(len(strings.Join(lines, "\n")))
}
