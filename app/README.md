# Lexe app

This directory contains the Lexe mobile app UI, which is written in
Dart+Flutter.

## Dev setup

After following these setup steps, you'll be able to test and run the Lexe app,
on device or simulator, for both Android and iOS.


### Android setup

We'll install Java and the android SDKs via CLI, as it's more repeatable.

#### Install Java (via `sdkman`)

`sdkman` is like `rustup` but for Java. Unfortunately, the Java ecosystem is a
bit more... convoluted than the Rust ecosystem, so we have to choose a JDK
"distribution" to install. I just picked whatever <https://whichjdk.com/>
recommended (`11.0.7-tem`) and it seems to work.

Ensure your `.bashrc` contains these lines:

```bash
export SDKMAN_DIR="$HOME/.local/sdkman"

[[ -s "$SDKMAN_DIR/bin/sdkman-init.sh" ]] \
	&& source "$SDKMAN_DIR/bin/sdkman-init.sh"
```

Actually install `sdkman` and Java:

```bash
# download the sdkman install script
$ cd ~/.local/
$ curl --proto '=https' --tlsv1.3 -sSf "https://get.sdkman.io?rcupdate=false" > sdkman-install.sh
$ sha256sum sdkman-install.sh
419762944a301418a6c68021c5c864f54a3ce3e013571bd38da448439695f582

# install sdkman
$ chmod a+x ./sdkman-install.sh
$ ./sdkman-install.sh
$ rm sdkman-install.sh

# reload $PATH
$ source ~/.bashrc

# install JDK
$ sdk list java
$ sdk install java 11.0.17-tem

# sanity check
$ which javac
~/.local/sdkman/candidates/java/current/bin/javac
$ javac --version
javac 11.0.17
```

#### Install android `cmdline-tools`

This step will give us the android `sdkmanager` from the `cmdline-tools`
"package", which we'll use to actually install the android SDKs.

You can find the latest `cmdline-tools` download links here:
<https://developer.android.com/studio#command-line-tools-only>

```bash
$ cd ~/.local/

# (linux) download
$ wget https://dl.google.com/android/repository/commandlinetools-linux-9123335_latest.zip -O commandlinetools.zip
$ sha256sum commandlinetools.zip
0bebf59339eaa534f4217f8aa0972d14dc49e7207be225511073c661ae01da0a

# (macOS) download
$ wget https://dl.google.com/android/repository/commandlinetools-mac-9123335_latest.zip -O commandlinetools.zip
$ sha256sum commandlinetools.zip
d0192807f7e1cd4a001d13bb1e5904fc287b691211648877258aa44d1fa88275

# see zip file structure
$ unzip -l commandlinetools.zip
cmdline-tools/bin/...
cmdline-tools/lib/...
cmdline-tools/...

# install into ~/.local/android/cmdline-tools/latest
$ mkdir -p android/cmdline-tools/latest
$ unzip commandlinetools.zip -d android/cmdline-tools/latest
$ mv android/cmdline-tools/latest/cmdline-tools/* android/cmdline-tools/latest/
$ rmdir android/cmdline-tools/latest/cmdline-tools
$ rm commandlinetools.zip
```

Ensure `.bashrc` contains these lines:

```bash
# Android
export ANDROID_HOME=$HOME/.local/android
ANDROID_SDK_VERSION=33.0.1
ANDROID_PATH=$ANDROID_HOME/cmdline-tools/latest/bin
ANDROID_PATH=$ANDROID_PATH:$ANDROID_HOME/build-tools/$ANDROID_SDK_VERSION
ANDROID_PATH=$ANDROID_PATH:$ANDROID_HOME/platform-tools

export PATH=$PATH:$ANDROID_PATH
```

Finally check our install:

```bash
# reload $PATH
$ source ~/.bashrc

# sanity check
$ which sdkmanager
~/.local/android/cmdline-tools/latest/bin/sdkmanager
$ sdkmanager --version
8.0
```

#### Install android SDKs via `sdkmanager`

```bash
# let's blindly accept every license : )
$ yes | sdkmanager --licenses

# check out the available SDK packages
$ sdkmanager --list
add-ons;addon-google_apis-google-24
build-tools;33.0.1
cmake;3.22.1
cmdline-tools;latest
emulator
extras;android;m2repository
extras;google;auto
extras;google;google_play_services
extras;google;instantapps
extras;google;m2repository
extras;google;market_apk_expansion
extras;google;market_licensing
extras;google;simulators
extras;google;webdriver
extras;m2repository;com;android;support;constraint;constraint-layout-solver;1.0.0
extras;m2repository;com;android;support;constraint;constraint-layout-extras;m2repository;com;android;support;constraint;constraint-layout-solver;1.0.2
extras;m2repository;com;android;support;constraint;constraint-layout;1.0.2
ndk-bundle
ndk;25.1.8937393
patcher;v4
platform-tools
platforms;android-33
platforms;android-TiramisuPrivacySandbox
skiaparser;3
sources;android-33
system-images;android-33;google_apis;arm64-v8a
system-images;android-33;google_apis;x86_64
system-images;android-33;google_apis_playstore;arm64-v8a
system-images;android-33;google_apis_playstore;x86_64
system-images;android-TiramisuPrivacySandbox;google_apis_playstore;arm64-v8a
system-images;android-TiramisuPrivacySandbox;google_apis_playstore;x86_64

# .. this goes on for a while

# install these. you may need to update the version #'s.
$ sdkmanager --install \
	"build-tools;33.0.1" \
	"platform-tools" \
	"platforms;android-33" \
	"sources;android-33" \
	"ndk;25.1.8937393"

# sanity check
$ adb version
Android Debug Bridge version 1.0.41
Version 33.0.3-8952118
Installed as ~/.local/android/platform-tools/adb
```

#### (linux only) USB debugging setup

If you only plan to use Wireless debugging, you can skip this section. Otherwise
you'll need to jump through a few extra hoops on linux machines:

Add your user to the `plugdev` group:

```bash
$ sudo usermod -aG plugdev $LOGNAME
```

Install `udev` rules:

```bash
$ sudo apt install android-sdk-platform-tools-common
```

Restart your machine. `adb devices` should now pick up any connected devices.


### (macOS only) iOS setup

#### (Apple Silicon only) Install Rosetta

```bash
$ sudo softwareupdate --install-rosetta --agree-to-license
```

#### Install Xcode

Either download the app directly from <https://developer.apple.com/xcode/download/>
or install it from the Mac App Store.

Once installed, run

```bash
$ sudo xcode-select \
	--switch /Applications/Xcode.app/Contents/Developer
$ sudo xcodebuild -runFirstLaunch
```

#### Install CocoaPods

From the "Sudo-less Install" section:
<https://guides.cocoapods.org/using/getting-started.html#installation>

Ensure your `.bashrc` contains something like:

```bash
export GEM_HOME=$HOME/.local/gem
GEM_BIN=$GEM_HOME/bin

export PATH=$PATH:$GEM_BIN
```

Install the `cocoapods` gem:

```bash
# sanity check
$ gem --version
3.0.3.1

$ gem install cocoapods

# sanity check
$ pod --version
1.11.3
```

#### Ensure the iOS Simulator app works

Search for "Simulator" in Spotlight and then open it. An emulated iPhone should pop up after a minute or so.

For me (Philip) the Simulator app wasn't available initially--my guess is that
Xcode installs it lazily, on an as-needed basis. To fix this, I had to open
and run a random sample iOS app in Xcode.


### (Pop\_OS! only?) install `libstdc++-12-dev`

If you want to run flutter linux desktop apps:

```bash
$ sudo apt install libstdc++-12-dev
```


### Flutter setup

From the setup instructions online: <https://docs.flutter.dev/get-started/install>

Ensure your `.bashrc` contains something like:

```bash
FLUTTER_HOME=$HOME/.local/flutter
export PATH=$PATH:$FLUTTER_HOME/bin
```

Installing `flutter` means just cloning their repo. Make sure you _don't_ do a
shallow clone (`--depth=1`); that breaks the flutter upgrades.

```bash
$ git clone \
    --branch="stable" \
	https://github.com/flutter/flutter.git \
	~/.local/flutter
```

Disable their pesky telemetry : )

```bash
$ flutter config --suppress-analytics --no-analytics
$ dart --disable-analytics
```

Run `flutter doctor` to check your install:

```bash
$ flutter doctor

[✓] Flutter (Channel beta, 3.7.0-1.5.pre, on Pop!_OS 22.04 LTS 6.0.6-76060006-generic, locale en_US.UTF-8)
[✓] Android toolchain - develop for Android devices (Android SDK version 33.0.1)
[✗] Chrome - develop for the web (Cannot find Chrome executable at google-chrome)
    ! Cannot find Chrome. Try setting CHROME_EXECUTABLE to a Chrome executable.
[✓] Linux toolchain - develop for Linux desktop
[!] Android Studio (not installed)
[✓] Connected device (1 available)
[✓] HTTP Host Availability

! Doctor found issues in 2 categories.
```

This output is from my Pop!\_OS linux desktop. Since we don't care about flutter
web (it's trash) and Android Studio is not actually necessary (I don't use it),
we're all good to go!

Now let's check that flutter picks up any connected devices or simulators. If
you have an actual Android or iOS device, make sure they have Debugging/Dev mode
turned on and are connected to your machine.

On my Pop!\_OS linux desktop:

```bash
$ flutter devices
2 connected devices:

Pixel 5a (mobile) • android-arm64 • Android 13 (API 33)
Linux (desktop) • linux-x64 • Pop!_OS 22.04 LTS
```

On my M1 MBP:

```bash
$ flutter devices
3 connected devices:

Pixel 5a (mobile) • android-arm64 • Android 13 (API 33)
iPhone 14 Pro (mobile) • ios • iOS-16-2 (simulator)
macOS (desktop) • macos • darwin-arm64 • macOS 13.1 darwin-arm
```

#### Test your flutter install on a sample app

```bash
$ flutter create --platforms ios,android,windows,linux,macos my_app
$ cd my_app

# this should build, install, and launch the sample app on your
# phone/simulator/desktop
$ flutter run -d pixel
$ flutter run -d iphone
$ flutter run -d mac
$ flutter run -d linux
```


### (nvim) Editor setup

For all the `nvim` chads out there using `coc.nvim`, just do:

```vim
:CocInstall coc-flutter
```

Set `"dart.showTodos": false` in your `coc-settings.json`.


## Dev workflow

To run the app in debug mode, run the following in the `app/` directory.

```bash
$ flutter run
```

While running in debug mode, you can hit `r` to hot reload the UI. If you make
changes any `StatefulWidget`s or other logic before `runApp(..)`, you'll need to
hot restart with `R`.

When evaluating UI performance, run the app in profiling mode. This enables just
enough debug info to be useful without slowing everything down like debug mode.

```bash
$ flutter run --profile
```

Run unit tests:

```bash
$ flutter test
```

Run integration tests on device or emulator:

```bash
$ flutter drive
```
