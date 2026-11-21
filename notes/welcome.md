# Welcome

This page is a Markdown file (`notes/welcome.md`) turned into HTML.
It also works as a cheat sheet: everything below is **supported** by the interpreter.

## Text

Some **bold**, some *italic*, some ~~strikethrough~~, some `inline code`
and a [link](https://en.wikipedia.org/wiki/Markdown "Markdown on Wikipedia").
A bare URL works too: https://example.com

> A quote,
> over several lines, with some *style* inside.

## Lists

- A bullet
- Another one
  - nested
  - again
- The end

1. First
2. Second
   - with a sub-list

- [x] Write the interpreter
- [ ] Write my notes

## Code

```js
function greet(name) {
  return `Hello ${name}!`;
}
```

## Table

| Syntax      | Result      | Right-aligned |
|-------------|:-----------:|--------------:|
| `**bold**`  | **bold**    |             1 |
| `*italic*`  | *italic*    |            22 |

## Math

Inline math between single dollars: $E = mc^2$. Prices like $5 and $10 stay plain text.

$$
\int_0^1 x^2 \, dx = \frac{1}{3}
$$

```math
\sum_{i=1}^{n} i = \frac{n(n+1)}{2}
```

---

Raw HTML is escaped: <script>alert("no")</script> is shown as-is.
