# Resume fixtures

Realistic resumes for the offline tests. The people and companies are
fictional.

| File | Made from | Exercises |
| --- | --- | --- |
| `marina_costa.pdf` | `marina_costa.html`, printed by headless Chromium | two pages, running header and "Page N of M" footer, bullets drawn as shapes (no bullet characters), right-aligned dates, two roles under one company, overlapping dates, a current role, an undated freelance role, projects, education, skills, languages |
| `marina_costa_v2.pdf` | `marina_costa_v2.html` | the same resume, changed: promoted title, one bullet reworded, one removed, the freelance role gone, an internship added, a new skill |
| `positioned_words.pdf` | `positioned_words.py` | a TeX-style file: every word placed individually with no space characters, bullet glyphs, a wrapped bullet, a footer with page numbers |
| `blank.pdf` | `blank.html` | a valid PDF with no text (as a scanned resume would be) |
| `truncated.pdf` | the first 6000 bytes of `marina_costa.pdf` | a damaged PDF |
| `not_a_pdf.pdf` | text | a file named `.pdf` that is not one |
| `ana_lima.md`, `ana_lima_v2.md` | written by hand | Markdown resumes, and a corrected re-import |
| `plain.txt` | written by hand | a plain-text resume with a different layout |

To regenerate the Chromium PDFs:

```bash
chrome --headless --no-pdf-header-footer --print-to-pdf=../marina_costa.pdf "file://$PWD/marina_costa.html"
```
