---
title: Test
---
# Hello, Markdown ✨

A paragraph with **bold**, *italic*, ***both***, ~~strike~~, `inline code`, a [link](https://example.com "Title"), and an autolink https://claude.ai. Also <kbd>Ctrl</kbd>+<kbd>O</kbd> and a snake_case_word that must not go italic.
Line two with a hard break  
and a third line.

> [!NOTE]
> Alerts work like on GitHub.

> A normal quote
> with two lines

## Lists

- Apples
- Bananas
  - nested **one**
  - nested two
    1. deep ordered
    2. deep ordered 2
- [x] done task
- [ ] open task

3. starts at three
4. four

- loose item

- another loose item

## Code

```js
// greet
const greet = (name) => `Hello, ${name}!`;
console.log(greet("world"), 42);
```

```python
def add(a, b):  # add
    return a + b
```

```diff
+ added line
- removed line
```

    indented code block

## Table

| Name | Qty | Price |
|:-----|:---:|------:|
| Tea  | 2   | $3.50 |
| Cake | 10  | $12   |

### Raw HTML

<p align="center"><b>centered</b> <img src="data:image/gif;base64,R0lGODlhAQABAAAAACw=" width="20"></p>

<details>
<summary>Click me</summary>

Hidden *markdown* content.

</details>

<script>alert('xss')</script>
[bad](javascript:alert(1))

Setext Heading
--------------

***

Footnote-ish ref [docs] and [text][id].

[docs]: https://example.com/docs
[id]: https://example.org
