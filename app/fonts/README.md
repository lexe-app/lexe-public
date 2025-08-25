# Lexe app fonts

This directory contains all the font assets used in lexe.app.

## Rebuilding InterVariable from master

```bash
$ git clone --filter=blob:none https://github.com/rsms/inter.git inter-font
$ cd inter-font

$ make -j all

# (macOS) install variable fonts in ~/Library/Fonts/Inter/
$ make install_var

# install variable fonts in repo
$ cp -v build/fonts/var/Inter*.ttf ~/dev/lexe/public/app/fonts/
```

## Hubot Sans

Download from <https://github.com/github/hubot-sans/tree/main/fonts/variable>
and place it in `app/fonts/Hubot-Sans.ttf`.

```bash
$ cp ~/Downloads/"HubotSans[slnt,wdth,wght].ttf" ~/dev/lexe/public/app/fonts/Hubot-Sans.ttf
```


## Rebuild `lexeicons.ttf` from raw SVGs

The <./lexeicons-svgs> directory contains raw, unprocessed SVG icons that we
use. These are currently sourced from: <https://simpleicons.org>.

We can't easily or efficiently render .svg icons in flutter, so we need to
process and collect these .svg icons into a .ttf font file, <./lexeicons.ttf>.

### Process

1. Open <https://fontello.com>.
2. Drag all the raw .svg icon files in the <./lexeicons-svgs> directory into the
   "Custom Icons" section on Fontello.
3. Select all "Custom Icons".
4. Name the font `lexeicons` and hit "Download webfont".
5. Unzip and extract `lexeicons.ttf` from the downloaded zip file to
   `public/app/fonts/lexeicons.ttf`.
6. Open <../lib/style.dart> and update the corresponding `LxIcons` codepoints
   to match the codepoints assigned by Fontello (under the "Customize Codes"
   tab).
