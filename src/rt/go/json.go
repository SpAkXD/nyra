
// ---- json: `json.str(v)` and `json.parse(text)` by the value's type ----

type nyJEncoder interface{ nyJEnc(b *strings.Builder) }

func nyJStrOf(v any) string {
	var b strings.Builder
	nyJEnc(&b, v)
	return b.String()
}

func nyJEnc(b *strings.Builder, v any) {
	switch x := v.(type) {
	case int64:
		b.WriteString(strconv.FormatInt(x, 10))
	case float64:
		if math.IsNaN(x) || math.IsInf(x, 0) {
			b.WriteString("null")
		} else {
			b.WriteString(nyNum(x))
		}
	case bool:
		b.WriteString(strconv.FormatBool(x))
	case rune:
		nyJEncStr(b, string(x))
	case string:
		nyJEncStr(b, x)
	case nyJEncoder:
		x.nyJEnc(b)
	}
}

func (a *Array[T]) nyJEnc(b *strings.Builder) {
	b.WriteByte('[')
	for i, x := range a.items {
		if i > 0 {
			b.WriteByte(',')
		}
		nyJEnc(b, any(x))
	}
	b.WriteByte(']')
}

func nyJEncStr(b *strings.Builder, s string) {
	b.WriteByte('"')
	for _, c := range s {
		switch {
		case c == '"':
			b.WriteString("\\\"")
		case c == '\\':
			b.WriteString("\\\\")
		case c == '\b':
			b.WriteString("\\b")
		case c == '\f':
			b.WriteString("\\f")
		case c == '\n':
			b.WriteString("\\n")
		case c == '\r':
			b.WriteString("\\r")
		case c == '\t':
			b.WriteString("\\t")
		case c < 0x20:
			fmt.Fprintf(b, "\\u%04x", c)
		default:
			b.WriteRune(c)
		}
	}
	b.WriteByte('"')
}

// nyJP: the parser; the text, the position, the nesting and the path to the value being read.
type nyJP struct {
	s           string
	i, depth    int
	path        []string
	line, col   int
}

func nyJParse[T any](text string, dec func(*nyJP) T, line, col int) T {
	p := &nyJP{s: text, line: line, col: col}
	v := dec(p)
	p.ws()
	if p.i < len(p.s) {
		p.syntax("text after the value")
	}
	return v
}

func (p *nyJP) syntax(what string) {
	end := p.i
	if end > len(p.s) {
		end = len(p.s)
	}
	line := strings.Count(p.s[:end], "\n") + 1
	nyFail("E0345", fmt.Sprintf("json.parse: invalid JSON at line %d: %s", line, what),
		"check the JSON text: it must be one value, with keys and strings in double quotes", p.line, p.col)
}

func (p *nyJP) typeErr(what string) {
	nyFail("E0345", "json.parse: expected "+what+" at $"+strings.Join(p.path, ""),
		"the JSON text must have the shape of the type it is read into", p.line, p.col)
}

func (p *nyJP) missing(field string) {
	nyFail("E0345", "json.parse: missing field \""+field+"\" at $"+strings.Join(p.path, ""),
		"the JSON object must have every field of the struct", p.line, p.col)
}

// peek: the byte at the position, or -1 at the end.
func (p *nyJP) peek() int {
	if p.i < len(p.s) {
		return int(p.s[p.i])
	}
	return -1
}

func (p *nyJP) ws() {
	for c := p.peek(); c == ' ' || c == '\t' || c == '\n' || c == '\r'; c = p.peek() {
		p.i++
	}
}

// start skips white space to the start of a value: a syntax error unless one can start here.
func (p *nyJP) start() byte {
	p.ws()
	c := p.peek()
	if c < 0 {
		p.syntax("unexpected end of the text")
	}
	if !strings.ContainsRune("{[\"tfn-0123456789", rune(c)) {
		p.syntax("expected a value")
	}
	return byte(c)
}

// open: `[` or `{`; true if a first element follows, false for an empty one (already closed).
func (p *nyJP) open(open byte, what string) bool {
	if p.start() != open {
		p.typeErr(what)
	}
	p.depth++
	if p.depth > 500 {
		p.syntax("nested too deeply")
	}
	p.i++
	p.ws()
	close := byte('}')
	if open == '[' {
		close = ']'
	}
	if p.peek() == int(close) {
		p.i++
		p.depth--
		return false
	}
	return true
}

// next: after an element; true at `,` (another one follows), false at the closing bracket.
func (p *nyJP) next(close byte) bool {
	p.ws()
	c := p.peek()
	if c == ',' {
		p.i++
		return true
	}
	if c == int(close) {
		p.i++
		p.depth--
		return false
	}
	if c < 0 {
		p.syntax("unexpected end of the text")
	}
	p.syntax("expected `,` or `" + string(rune(close)) + "`")
	return false
}

func (p *nyJP) hex(at int) int {
	if at+4 > len(p.s) {
		return -1
	}
	v, err := strconv.ParseUint(p.s[at:at+4], 16, 32)
	if err != nil || strings.ContainsAny(p.s[at:at+4], "+-") {
		return -1
	}
	return int(v)
}

// str reads a string; the position is at its `"`.
func (p *nyJP) str() string {
	var out strings.Builder
	p.i++
	for {
		c := p.peek()
		if c < 0 {
			p.syntax("unterminated string")
		}
		if c == '"' {
			p.i++
			return out.String()
		}
		if c < 0x20 {
			p.syntax("control character in a string")
		}
		if c != '\\' {
			out.WriteByte(byte(c))
			p.i++
			continue
		}
		p.i++
		e := p.peek()
		if e < 0 {
			p.syntax("unterminated string")
		}
		simple := map[int]rune{'"': '"', '\\': '\\', '/': '/', 'b': '\b', 'f': '\f', 'n': '\n', 'r': '\r', 't': '\t'}
		if r, ok := simple[e]; ok {
			out.WriteRune(r)
			p.i++
			continue
		}
		if e != 'u' {
			p.syntax("invalid escape")
		}
		cp := p.hex(p.i + 1)
		if cp < 0 || (cp >= 0xDC00 && cp <= 0xDFFF) {
			p.syntax("invalid escape")
		}
		p.i += 5
		if cp >= 0xD800 && cp <= 0xDBFF {
			lo := -1
			if strings.HasPrefix(p.s[p.i:], "\\u") {
				lo = p.hex(p.i + 2)
			}
			if lo < 0xDC00 || lo > 0xDFFF {
				p.syntax("invalid escape")
			}
			cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00)
			p.i += 6
		}
		out.WriteRune(rune(cp))
	}
}

// number reads a number: its text, and whether it has no fraction and no exponent.
func (p *nyJP) number() (string, bool) {
	start := p.i
	digit := func() bool { c := p.peek(); return c >= '0' && c <= '9' }
	whole := true
	if p.peek() == '-' {
		p.i++
	}
	if p.peek() == '0' {
		p.i++
	} else if digit() {
		for digit() {
			p.i++
		}
	} else {
		p.syntax("invalid number")
	}
	if p.peek() == '.' {
		p.i++
		if !digit() {
			p.syntax("invalid number")
		}
		for digit() {
			p.i++
		}
		whole = false
	}
	if c := p.peek(); c == 'e' || c == 'E' {
		p.i++
		if c := p.peek(); c == '+' || c == '-' {
			p.i++
		}
		if !digit() {
			p.syntax("invalid number")
		}
		for digit() {
			p.i++
		}
		whole = false
	}
	return p.s[start:p.i], whole
}

func (p *nyJP) literal() string {
	for _, w := range []string{"true", "false", "null"} {
		if strings.HasPrefix(p.s[p.i:], w) {
			p.i += len(w)
			return w
		}
	}
	p.syntax("expected a value")
	return ""
}

// key reads an object's key and the `:` after it.
func (p *nyJP) key() string {
	p.ws()
	switch p.peek() {
	case -1:
		p.syntax("unexpected end of the text")
	case '"':
	default:
		p.syntax("expected a string key")
	}
	k := p.str()
	p.ws()
	switch p.peek() {
	case -1:
		p.syntax("unexpected end of the text")
	case ':':
		p.i++
	default:
		p.syntax("expected `:`")
	}
	return k
}

// skip checks any value (an object's fields that the type does not have).
func (p *nyJP) skip() {
	c := p.start()
	switch {
	case c == '{' || c == '[':
		close := byte('}')
		if c == '[' {
			close = ']'
		}
		if p.open(c, "") {
			for {
				if c == '{' {
					p.key()
				}
				p.skip()
				if !p.next(close) {
					break
				}
			}
		}
	case c == '"':
		p.str()
	case c == '-' || (c >= '0' && c <= '9'):
		p.number()
	default:
		p.literal()
	}
}

func nyJInt(p *nyJP) int64 {
	c := p.start()
	if c != '-' && (c < '0' || c > '9') {
		p.typeErr("an int")
	}
	t, whole := p.number()
	v, err := strconv.ParseInt(t, 10, 64)
	if !whole || err != nil {
		p.typeErr("an int")
	}
	return v
}

func nyJFloat(p *nyJP) float64 {
	c := p.start()
	if c != '-' && (c < '0' || c > '9') {
		p.typeErr("a number")
	}
	t, _ := p.number()
	v, _ := strconv.ParseFloat(t, 64)
	return v
}

func nyJBool(p *nyJP) bool {
	c := p.start()
	if c != 't' && c != 'f' {
		p.typeErr("true or false")
	}
	return p.literal() == "true"
}

func nyJChar(p *nyJP) rune {
	if p.start() != '"' {
		p.typeErr("a one-character string")
	}
	s := p.str()
	if utf8.RuneCountInString(s) != 1 {
		p.typeErr("a one-character string")
	}
	r, _ := utf8.DecodeRuneInString(s)
	return r
}

func nyJStr(p *nyJP) string {
	if p.start() != '"' {
		p.typeErr("a string")
	}
	return p.str()
}

func nyJArr[T any](dec func(*nyJP) T) func(*nyJP) *Array[T] {
	return func(p *nyJP) *Array[T] {
		items := []T{}
		if p.open('[', "an array") {
			for {
				p.path = append(p.path, "["+strconv.Itoa(len(items))+"]")
				items = append(items, dec(p))
				p.path = p.path[:len(p.path)-1]
				if !p.next(']') {
					break
				}
			}
		}
		return &Array[T]{items: items}
	}
}
