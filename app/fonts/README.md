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
