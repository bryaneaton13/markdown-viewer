---
title: A quieter place for your Markdown
author: Your local workspace
tags: [reading, notes, documentation]
status: prototype
---
# A quieter place for your Markdown

A native, read-only workspace for the documents already on your Mac. **No importing. No accounts. No accidental edits.**

## Made for reading

Browse a folder on the left, or press **⌘P** to jump straight to a file. When you're ready to focus, **⌘B** puts the document front and center.

> Good tools get out of the way. Your writing should be the most interesting thing in the window.

### A few things to try

- Select and copy text across paragraphs.
- Press **⌘F** and search for “Markdown”.
- Expand **Properties** to inspect the YAML frontmatter.
- Switch to **Source**. It is read-only too.
- Follow the [local reading notes](notes/Reading.md), then go back with **⌘[**.

## Small details, properly rendered

| Feature | In the reader | In Finder |
| :--- | :--- | :--- |
| Markdown text | Native GPU rendering | Quick Look preview |
| Frontmatter | Collapsible properties | Collapsible properties |
| Files on disk | Never modified | Never modified |

```rust
fn main() {
    let purpose = "Read something worth keeping.";
    println!("{purpose}");
}
```

- [x] Open Markdown files
- [x] Keep the source untouched
- [ ] Take a moment to read

## Local images

![A small landscape illustration](landscape.png)

## Footnotes & links

Markdown is plain text, with a little structure.[^plain]

[Jump back to the start](#a-quieter-place-for-your-markdown) · [Markdown reference](https://commonmark.org/help/)

[^plain]: This footnote stays part of the document. No hosted service required.
