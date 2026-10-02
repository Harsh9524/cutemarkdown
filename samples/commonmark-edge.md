# CommonMark and GFM Edge Cases

A deliberately awkward document. Each section targets one parser or renderer pitfall. Where behavior is viewer-defined, the section says so.

Setext Heading, Level 1
=======================

Setext Heading, Level 2
-----------------------

A multi-line
setext heading
--------------

## Emphasis edge cases

- Snake case must not italicize: snake_case_word, __init__ is bold, but my_var_name and file_name.txt stay literal.
- Intraword asterisks do emphasize: 2*3*4 renders as 2, then italic 3, then 4. With spaces, 2 * 3 * 4 stays literal.
- Nested: **bold *nested italic* bold** and *italic **nested bold** italic* and ***bold italic***.
- Intraword: foo*bar*baz is emphasized, foo_bar_baz is not, and **foo**bar is bold then plain.
- Unmatched markers: a lone * star, a lone _ underscore, and 5 ** 3 = 125.
- Strikethrough: ~~deleted~~, ~single tilde~, ~~**bold and deleted**~~, and ~~not closed.

## Hard breaks

Two trailing spaces end this line  
and this line follows it.
A backslash ends this one\
and this follows it.
A plain newline here is only a soft break, so this should join the previous line.

## Escaped characters

\*not italic\*, \_not italic\_, \# not a heading, \[not a link\](nope), 1\. not a list, \`not code\`, \<div\> not html, a literal backslash \\, and a pipe \| in text.

Entities: &amp; &lt;b&gt; &copy; &#35; &#x1F680; &nbsp;(nbsp) &unknown;

## Links

Autolinks: <https://example.com/autolink> and <mailto:hello@example.com> and <hello@example.com>.

Bare URLs (GFM extended autolinks): www.example.com and https://example.com/path?q=1. (the final period is punctuation), and (https://example.com/in-parens) and https://en.wikipedia.org/wiki/Markdown_(disambiguation).

Reference-style: [CommonMark spec][cm], the [GFM spec], a [collapsed reference][], and an [undefined reference] that must stay literal.

Inline with a title: [example](https://example.com "Example Title") and angle-bracket destination [spaces](<https://example.com/a b>).

[cm]: https://spec.commonmark.org/ "CommonMark Spec"
[GFM spec]: https://github.github.com/gfm/
[collapsed reference]: https://example.com/collapsed

## Inline HTML (safe)

Press <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>P</kbd> to open the palette. Water is H<sub>2</sub>O, and E = mc<sup>2</sup>. A <mark>highlighted</mark> word, an <abbr title="HyperText Markup Language">HTML</abbr> abbreviation, and a forced<br>line break.

<details>
<summary>Click to expand (details/summary)</summary>

Hidden **markdown** content with a list:

- one
- two

</details>

## Dangerous HTML (must NOT execute)

None of the following may run script, load a frame, or show an alert. Dropping them or showing them as escaped text are both acceptable.

<script>alert('xss: script tag')</script>

<iframe src="https://example.com" width="300" height="100"></iframe>

[Click me: javascript: link](javascript:alert('xss: link'))

<img src="x" onerror="alert('xss: onerror')" alt="broken on purpose">

<a href="javascript:alert('xss: raw anchor')" onclick="alert('xss: onclick')">raw anchor</a>

<svg onload="alert('xss: svg')"></svg> <style>body { display: none !important }</style> trailing text after dangerous inline HTML.

## Code blocks

Indented code block (four spaces):

    fn main() {
        println!("indented, not fenced");
    }

Tilde fence with a language:

~~~python
print("tilde fence")
~~~

A four-backtick fence containing a three-backtick fence:

````markdown
Here is a fence inside a fence:

```js
console.log("nested");
```
````

Inline code with backticks inside: `` code with a ` backtick `` and ``` `` double ```.

## Block quotes

> Level 1 quote with a lazy
continuation line.
>
> - item in level 1
> - another item
>
> > Level 2 quote
> >
> > 1. ordered item in level 2
> > 2. second item
> >
> > > Level 3 quote containing code:
> > >
> > > ```python
> > > print("three levels deep")
> > > ```
> > >
> > > - list inside level 3
> > > - **bold** inside level 3

## Lists

Mixed markers (each change of marker starts a new list):

- dash one
- dash two
* star one
* star two
+ plus one
+ plus two

An ordered list that starts at 7:

7. Seventh
8. Eighth
9. Ninth
10. Tenth (two-digit marker)

A loose list (blank lines between items, multiple paragraphs):

- First item.

  Second paragraph of the first item.

- Second item.

      indented code inside a list item

- Third item with a nested tight list:
  - child a
  - child b

A tight list with a task list mixed in:

- [x] done
- [ ] not done
- plain item

## Thematic breaks

***

___

- - -

## Long unbreakable words

A bare URL of 150 characters (autolinked, no spaces): https://example.com/segment-one-segment-two-segment-three-segment-four-segment-five-segment-six-segment-seven-segment-eightsegment-one-segment-two-seg

A plain token of 150 characters (no spaces, no scheme): SupercalifragilisticSupercalifragilisticSupercalifragilisticSupercalifragilisticSupercalifragilisticSupercalifragilisticSupercalifragilisticSupercalif

## Tables

| Expression | Alignment | Notes |
| :--- | :---: | ---: |
| `a \| b` | center | pipe escaped inside code |
| a \| b | center | pipe escaped in text |
|  | empty first cell | right |
| last row | | |
| **bold** and `code` | ~~struck~~ | [link](https://example.com) |

## Headings

### Closing hashes ###

#### Trailing hashes and spaces ####   

#hashtag is not a heading because there is no space after the hash.

####### Seven hashes is not a heading either.

## The `render()` function and a [link](#)

### Duplicate

First one.

### Duplicate

Second one: anchors must be unique (for example duplicate, duplicate-1, duplicate-2).

### Duplicate

Third one.

## Emoji

Shortcodes (viewer may or may not support them): :rocket: :tada: :warning: :white_check_mark: :+1: :thumbsup: :heart: :not_a_real_emoji:

Literal Unicode emoji for comparison: 🚀 🎉 ⚠️ ✅ 👍 ❤️ 👩‍💻 🏳️‍🌈

## Non-Latin text

Arabic (right-to-left): مرحبا بالعالم! هذا نص تجريبي لاختبار عرض المستندات بلغات متعددة. Hindi: नमस्ते दुनिया! यह कई भाषाओं में दस्तावेज़ दिखाने का एक परीक्षण है। Chinese: 你好,世界!这是一段用于测试多语言文档渲染的示例文字,没有空格也应该正确换行。 Japanese: こんにちは世界!これは多言語ドキュメントの表示をテストするためのサンプルです。 Cyrillic: Привет, мир! Это пример текста для проверки отображения на разных языках.

- العربية: **غامق** و *مائل*
- हिन्दी: **मोटा** और *तिरछा*
- 中文:**粗体**和*斜体*
- 日本語:**太字**と*斜体*
- Русский: **жирный** и *курсив*

## Final paragraph

This paragraph ends the file without a trailing newline problem, and contains a link to the [top](#commonmark-and-gfm-edge-cases).
