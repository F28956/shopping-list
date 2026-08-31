# The public site

Four hand-written HTML pages, one stylesheet, one small script. No build step, no
framework, no dependencies — it is uploaded as it stands.

That is a deliberate choice rather than laziness. The site exists to explain a program
somebody is about to run on their own machine; a page that needs a toolchain to produce a
paragraph of text is a page that stops being editable the day the toolchain rots. The
whole of it is under three thousand lines including the CSS.

## Layout

| File | What it is for |
|---|---|
| `index.html` | What the thing is, in the order somebody meets it: what it does, whose server, which devices |
| `install.html` | The long one. Downloads, whether you need a server at all, setting one up, HTTPS, every setting, running it as a service, pointing an app at it |
| `privacy.html` | What is stored, what is sent, what stays on the device, and what is not done |
| `support.html` | Getting connected, what to do when something is wrong, how to send a log, deleting your data |
| `styles.css` | All of the styling, for all four pages |
| `app.js` | One animation, described below |
| `icon.svg`, `icon-180.png` | Favicon and Apple touch icon |
| `social.png` | The Open Graph card |

The icons and the card are generated from `branding/`, which holds the logo and the
store icons at their required sizes.

## Design

**Dark, and it says so.** `color-scheme: dark` is declared in the head rather than left
to a media query, because the pages are written for one palette and a half-applied light
mode reads as a bug.

**One animated moment.** On the front page a line types itself — `2 kg apples` — and
becomes an item, because quick-add is the thing that is hard to describe and obvious to
watch. `app.js` does that and nothing else. It checks `prefers-reduced-motion` first and,
for anyone who has asked for less, renders the finished state immediately rather than
skipping to a blank one. The typing interval is jittered, because a person typing is
uneven and a metronome reads as a machine.

**Every page has a skip link and a single `<main>`.** The install page in particular is
long enough that landing at the top of it without a way past the masthead is a real cost.

**Nothing is fetched from anywhere.** No fonts, no analytics, no CDN. A page that
explains why the list never leaves your server should not phone somewhere while it does
it, and the privacy page makes a claim about this website that the markup has to keep.

## Publishing

Copy the directory. `release/*.sh` write their builds into `release/out/install/`, which
is the directory the download links on `install.html` point at — so a release and the
site are uploaded to the same place, and the page indexes what is actually there.

See [release/README.md](../release/README.md) for producing the builds themselves.
