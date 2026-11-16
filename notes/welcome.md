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

---

Raw HTML is escaped: <script>alert("no")</script> is shown as-is.
