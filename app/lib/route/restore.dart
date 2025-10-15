/// The wallet restore UI flow.
library;

import 'dart:async' show unawaited;

import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/form.dart' as form;
import 'package:app_rs_dart/ffi/gdrive.dart'
    show GDriveClient, GDriveRestoreCandidate;
import 'package:app_rs_dart/ffi/types.dart' show Config, RootSeed;
import 'package:flutter/material.dart';
import 'package:flutter_markdown/flutter_markdown.dart' show MarkdownBody;
import 'package:lexeapp/components.dart'
    show
        AnimatedFillButton,
        ErrorMessage,
        ErrorMessageSection,
        HeadingText,
        LxBackButton,
        LxCloseButton,
        LxCloseButtonKind,
        LxFilledButton,
        MultistepFlow,
        ScrollableSinglePageBody,
        SeedWord,
        SeedWordsCard,
        SubheadingText,
        baseInputDecoration;
import 'package:lexeapp/gdrive_auth.dart' show GDriveAuth, GDriveServerAuthCode;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/style.dart'
    show Fonts, LxColors, LxIcons, LxRadius, LxTheme, Space;

/// A tiny interface so we can mock the [AppHandle.restore] call in design mode.
abstract interface class RestoreApi {
  static const RestoreApi prod = _ProdRestoreApi._();

  Future<FfiResult<AppHandle>> restore({
    required Config config,
    String? googleAuthCode,
    required RootSeed rootSeed,
  });
}

class _ProdRestoreApi implements RestoreApi {
  const _ProdRestoreApi._();

  @override
  Future<FfiResult<AppHandle>> restore({
    required Config config,
    String? googleAuthCode,
    required RootSeed rootSeed,
  }) => Result.tryFfiAsync(
    () => AppHandle.restore(
      config: config,
      googleAuthCode: googleAuthCode,
      rootSeed: rootSeed,
    ),
  );
}

class RestorePage extends StatelessWidget {
  const RestorePage({
    super.key,
    required this.config,
    required this.gdriveAuth,
    required this.restoreApi,
  });

  final Config config;
  final GDriveAuth gdriveAuth;
  final RestoreApi restoreApi;

  Future<void> onGDrivePressed(BuildContext context) async {
    final AppHandle? flowResult = await Navigator.of(context).push(
      MaterialPageRoute(
        builder: (_) => RestoreGDriveAuthPage(
          config: this.config,
          gdriveAuth: this.gdriveAuth,
          restoreApi: this.restoreApi,
        ),
      ),
    );
    if (flowResult == null || !context.mounted) return;

    unawaited(Navigator.of(context).maybePop(flowResult));
  }

  Future<void> onSeedPhrasePressed(BuildContext context) async {
    final AppHandle? flowResult = await Navigator.of(context).push(
      MaterialPageRoute(
        builder: (_) => RestoreSeedPhrasePage(
          config: this.config,
          restoreApi: this.restoreApi,
        ),
      ),
    );
    if (flowResult == null) return;
    if (!context.mounted) return;
    unawaited(Navigator.of(context).maybePop(flowResult));
  }

  @override
  Widget build(BuildContext context) => MultistepFlow<AppHandle?>(
    builder: (_) => Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
      ),
      body: ScrollableSinglePageBody(
        body: [
          const Icon(
            LxIcons.nodeInfo,
            size: Space.s900,
            weight: 300,
            opticalSize: 48,
            grade: -50,
          ),
          MarkdownBody(
            data: '''
# Restore Wallet

Already have a Lexe Wallet?
Connect your Google Drive to restore from an existing Lexe
Wallet backup or use your Seed Phrase.
''',
            styleSheet: LxTheme.markdownStyle,
          ),
        ],
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: Column(
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              LxFilledButton(
                onTap: () => this.onGDrivePressed(context),
                label: const Text("Restore from Google Drive"),
                icon: const Icon(LxIcons.next),
                style: FilledButton.styleFrom(
                  backgroundColor: LxColors.foreground,
                  foregroundColor: LxColors.background,
                  iconColor: LxColors.background,
                ),
              ),
              const SizedBox(height: Space.s400),
              LxFilledButton(
                onTap: () => this.onSeedPhrasePressed(context),
                label: const Text("Restore from Seed Phrase"),
                icon: const Icon(LxIcons.next),
              ),
            ],
          ),
        ),
      ),
    ),
  );
}

/// First we need the user to authorize the app's access to their GDrive in
/// order to locate their wallet backups.
class RestoreGDriveAuthPage extends StatefulWidget {
  const RestoreGDriveAuthPage({
    super.key,
    required this.config,
    required this.gdriveAuth,
    required this.restoreApi,
  });

  final Config config;
  final GDriveAuth gdriveAuth;
  final RestoreApi restoreApi;

  @override
  State<RestoreGDriveAuthPage> createState() =>
      _RestoreGDriveAuthPageStateState();
}

class _RestoreGDriveAuthPageStateState extends State<RestoreGDriveAuthPage> {
  final ValueNotifier<bool> isRestoring = ValueNotifier(false);
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  @override
  void dispose() {
    this.isRestoring.dispose();
    this.errorMessage.dispose();
    super.dispose();
  }

  Future<void> onAuthPressed() async {
    if (this.isRestoring.value) return;

    this.isRestoring.value = true;
    try {
      await this.onAuthPressedInner();
    } finally {
      if (this.mounted) this.isRestoring.value = false;
    }
  }

  Future<void> onAuthPressedInner() async {
    // Hide error message
    this.errorMessage.value = null;

    // Ask user to auth with Google Drive.
    final Result<(GDriveClient, GDriveServerAuthCode)?, Exception>
    authResult = (await this.widget.gdriveAuth.tryAuth()).andThen((client) {
      if (client == null) return const Ok(null);
      final serverAuthCode = client.serverCode();
      if (serverAuthCode == null) {
        return Err(Exception("GDrive auth didn't return a server auth code"));
      }

      return Ok((client, GDriveServerAuthCode(serverAuthCode: serverAuthCode)));
    });
    if (!this.mounted) return;

    final GDriveServerAuthCode serverAuthCode;
    final GDriveClient gdriveClient;
    switch (authResult) {
      case Ok(:final ok):
        // user canceled. they might want to try again, so don't pop yet.
        if (ok == null) return;
        (gdriveClient, serverAuthCode) = ok;
      case Err(:final err):
        final errStr = err.toString();
        error("restore: Failed to auth user with GDrive: $errStr");
        this.errorMessage.value = ErrorMessage(
          title: "There was an error connecting your Google Drive",
          message: errStr,
        );
        return;
    }

    info("restore: authed with gdrive");

    // Try to locate any google drive backups for the current env.
    final config = this.widget.config;
    final findResult = await Result.tryFfiAsync(
      () => gdriveClient.intoRestoreClient().findRestoreCandidates(
        deployEnv: config.deployEnv,
        network: config.network,
        useSgx: config.useSgx,
      ),
    );
    if (!this.mounted) return;

    final List<GDriveRestoreCandidate> candidates;
    switch (findResult) {
      case Ok(:final ok):
        candidates = ok;
      case Err(:final err):
        error("restore: ${err.message}");
        this.errorMessage.value = ErrorMessage(
          title: "There was an error locating your Google Drive backup",
          message: err.message,
        );
        return;
    }

    final candidatesDbg = candidates
        .map((x) => x.userPk())
        .toList(growable: false);
    info("restore: found candidate UserPks: $candidatesDbg");

    // We authed, but there were no backups :(
    if (candidates.isEmpty) {
      warn("No backups in Google Drive");
      this.errorMessage.value = const ErrorMessage(
        title: "We couldn't find any Lexe Wallet backups for this account",
      );
      return;
    }

    // Either (1) goto password prompt if only one candidate, or (2) ask user to
    // choose which wallet first.
    final AppHandle? flowResult = await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (_) {
          // (normal case): Only one backup, open the password prompt page directly.
          if (candidates.length == 1) {
            return RestorePasswordPage(
              config: this.widget.config,
              restoreApi: this.widget.restoreApi,
              serverAuthCode: serverAuthCode,
              candidate: candidates.single,
            );
          } else {
            // It's possible (esp. for Lexe devs) to have multiple wallets for a single
            // gdrive account. Open a page to ask the user which UserPk they want to
            // restore.
            //
            // TODO(phlip9): UserPk isn't really a user-facing ID, so asking people to
            // choose by UserPk is definitely suboptimal. Figure out some kind of wallet
            // nickname system or something.
            return RestoreChooseWalletPage(
              config: this.widget.config,
              restoreApi: this.widget.restoreApi,
              serverAuthCode: serverAuthCode,
              candidates: candidates,
            );
          }
        },
      ),
    );
    if (flowResult == null) return;
    if (!this.mounted) return;

    info("restore: successful restore");

    // ignore: use_build_context_synchronously
    unawaited(Navigator.of(this.context).maybePop(flowResult));
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
      ),
      body: ScrollableSinglePageBody(
        body: [
          // Big Google Drive icon
          const Icon(
            LxIcons.gdrive,
            size: Space.s900,
            weight: 300,
            opticalSize: 48,
            grade: -50,
          ),
          MarkdownBody(
            data: '''
# Restore wallet from Google Drive

Connect your Google Drive to restore from an existing Lexe Wallet backup.

- **Your wallet backup is encrypted**. You'll need your backup password in a moment.
- Lexe cannot access any files in your Drive.
''',
            styleSheet: LxTheme.markdownStyle,
          ),

          // Error message
          Padding(
            padding: const EdgeInsets.only(top: Space.s500),
            child: ValueListenableBuilder(
              valueListenable: this.errorMessage,
              builder: (_context, errorMessage, _widget) =>
                  ErrorMessageSection(errorMessage),
            ),
          ),
        ],
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: ValueListenableBuilder(
            valueListenable: this.isRestoring,
            builder: (context, isRestoring, widget) => AnimatedFillButton(
              onTap: this.onAuthPressed,
              loading: isRestoring,
              label: const Text("Connect Google Drive"),
              icon: const Icon(LxIcons.next),
              style: FilledButton.styleFrom(
                backgroundColor: LxColors.foreground,
                foregroundColor: LxColors.background,
                iconColor: LxColors.background,
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// In rare cases, the user might have multiple wallets backed up in this gdrive
/// account. Ask them to choose one.
class RestoreChooseWalletPage extends StatefulWidget {
  const RestoreChooseWalletPage({
    super.key,
    required this.candidates,
    required this.serverAuthCode,
    required this.config,
    required this.restoreApi,
  });

  final Config config;
  final RestoreApi restoreApi;
  final GDriveServerAuthCode serverAuthCode;
  final List<GDriveRestoreCandidate> candidates;

  @override
  State<RestoreChooseWalletPage> createState() =>
      _RestoreChooseWalletPageState();
}

class _RestoreChooseWalletPageState extends State<RestoreChooseWalletPage> {
  Future<void> selectCandidate(final GDriveRestoreCandidate candidate) async {
    info("restore: chose UserPk: ${candidate.userPk()}");

    // Goto password prompt.
    final AppHandle? flowResult = await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (_) => RestorePasswordPage(
          config: this.widget.config,
          serverAuthCode: this.widget.serverAuthCode,
          candidate: candidate,
          restoreApi: this.widget.restoreApi,
        ),
      ),
    );
    if (flowResult == null) return;
    if (!this.mounted) return;

    unawaited(Navigator.of(this.context).maybePop(flowResult));
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
        actions: const [
          LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
          SizedBox(width: Space.s400),
        ],
      ),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "Choose wallet to restore"),
          const SubheadingText(text: "Wallets are listed by User public key"),
          const SizedBox(height: Space.s600),

          // List candidates (by UserPk)
          Column(
            mainAxisSize: MainAxisSize.min,
            mainAxisAlignment: MainAxisAlignment.start,
            children: this.widget.candidates
                .map(
                  // TODO(phlip9): add "created on XXX" subtitle to help differentiate?
                  (candidate) => ListTile(
                    title: Text(candidate.userPk()),
                    trailing: const Icon(LxIcons.nextSecondary),
                    onTap: () => this.selectCandidate(candidate),
                    contentPadding: EdgeInsets.zero,
                  ),
                )
                .toList(),
          ),
        ],
      ),
    );
  }
}

/// Ask the user for their backup password to decrypt the root seed backup.
class RestorePasswordPage extends StatefulWidget {
  const RestorePasswordPage({
    super.key,
    required this.candidate,
    required this.serverAuthCode,
    required this.config,
    required this.restoreApi,
  });

  final Config config;
  final RestoreApi restoreApi;
  final GDriveServerAuthCode serverAuthCode;
  final GDriveRestoreCandidate candidate;

  @override
  State<RestorePasswordPage> createState() => _RestorePasswordPageState();
}

class _RestorePasswordPageState extends State<RestorePasswordPage> {
  final GlobalKey<FormFieldState<String>> passwordFieldKey = GlobalKey();
  final ValueNotifier<bool> isRestoring = ValueNotifier(false);
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  @override
  void dispose() {
    this.isRestoring.dispose();
    this.errorMessage.dispose();
    super.dispose();
  }

  Future<void> onSubmit() async {
    if (this.isRestoring.value) return;

    // Hide error message
    this.errorMessage.value = null;

    final fieldState = this.passwordFieldKey.currentState!;
    final password = fieldState.value;
    if (password == null || !fieldState.validate()) return;

    final RootSeed rootSeed;
    switch (this.tryDecrypt(fieldState.value)) {
      case Ok(:final ok):
        rootSeed = ok;
      case Err():
        return;
    }

    this.isRestoring.value = true;
    try {
      await this.onSubmitInner(rootSeed);
    } finally {
      if (this.mounted) this.isRestoring.value = false;
    }
  }

  Future<void> onSubmitInner(RootSeed rootSeed) async {
    info("restore: recovered root seed");

    final restoreApi = this.widget.restoreApi;
    final config = this.widget.config;
    final serverAuthCode = this.widget.serverAuthCode.serverAuthCode;

    final result = await restoreApi.restore(
      config: config,
      googleAuthCode: serverAuthCode,
      rootSeed: rootSeed,
    );
    if (!this.mounted) return;

    final AppHandle flowResult;
    switch (result) {
      case Ok(:final ok):
        flowResult = ok;
      case Err(:final err):
        error("restore: AppHandle.restore failed: $err");
        this.errorMessage.value = ErrorMessage(
          title: "Error restoring wallet",
          message: err.message,
        );
        return;
    }

    unawaited(Navigator.of(this.context).maybePop(flowResult));
  }

  Result<RootSeed, String> tryDecrypt(final String? password) {
    if (password == null || password.isEmpty) return const Err("");
    return Result.tryFfi(
      () => this.widget.candidate.tryDecrypt(password: password),
    ).mapErr((err) {
      warn("restore: decrypt: ${err.message}");
      return "Invalid password";
    });
  }

  @override
  Widget build(BuildContext context) {
    final textFieldStyle = Fonts.fontUI.copyWith(
      fontSize: Fonts.size700,
      fontVariations: [Fonts.weightMedium],
      fontFeatures: [Fonts.featDisambugation],
      letterSpacing: -0.5,
    );

    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
        actions: const [
          LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
          SizedBox(width: Space.s400),
        ],
      ),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "Enter wallet backup password"),
          const SizedBox(height: Space.s200),
          const SubheadingText(
            text: "This password was set when the wallet was first created",
          ),
          const SizedBox(height: Space.s600),

          // Password field
          TextFormField(
            key: this.passwordFieldKey,
            autofocus: true,
            textInputAction: TextInputAction.done,
            validator: (password) => this.tryDecrypt(password).err,
            onEditingComplete: this.onSubmit,
            decoration: baseInputDecoration.copyWith(hintText: "Password"),
            obscureText: true,
            style: textFieldStyle,
          ),

          // Error message
          Padding(
            padding: const EdgeInsets.only(top: Space.s500),
            child: ValueListenableBuilder(
              valueListenable: this.errorMessage,
              builder: (_context, errorMessage, _widget) =>
                  ErrorMessageSection(errorMessage),
            ),
          ),
        ],
        // Restore ->
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: ValueListenableBuilder(
            valueListenable: this.isRestoring,
            builder: (context, isRestoring, widget) => AnimatedFillButton(
              onTap: this.onSubmit,
              loading: isRestoring,
              label: const Text("Restore"),
              icon: const Icon(LxIcons.next),
              style: FilledButton.styleFrom(
                backgroundColor: LxColors.moneyGoUp,
                foregroundColor: LxColors.grey1000,
                iconColor: LxColors.grey1000,
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class RestoreSeedPhrasePage extends StatefulWidget {
  const RestoreSeedPhrasePage({
    super.key,
    required this.config,
    required this.restoreApi,
  });

  final Config config;
  final RestoreApi restoreApi;

  @override
  State<RestoreSeedPhrasePage> createState() => _RestoreSeedPhrasePageState();
}

class _RestoreSeedPhrasePageState extends State<RestoreSeedPhrasePage> {
  final TextEditingController textController = TextEditingController();
  final FocusNode textFocusNode = FocusNode();
  final ValueNotifier<List<String>> mnemonicWords = ValueNotifier([]);
  final ValueNotifier<List<String>> suggestions = ValueNotifier([]);
  final ValueNotifier<bool> isRestoring = ValueNotifier(false);
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);
  static const amountWords = 24;

  @override
  void dispose() {
    textController.dispose();
    textFocusNode.dispose();
    mnemonicWords.dispose();
    suggestions.dispose();
    isRestoring.dispose();
    errorMessage.dispose();
    super.dispose();
  }

  void onTextChanged(String value) {
    final word = value.trim().toLowerCase();
    if (value.endsWith(' ')) textController.text = value.trim();

    if (word.isEmpty) {
      suggestions.value = [];
      return;
    }

    if (value.endsWith(' ') && this.isValidWord(word)) {
      this.onWordSelected(word);
      return;
    }

    this.suggestions.value = form.suggestMnemonicWords(prefix: word, take: 4);
  }

  void onWordSelected(String word) {
    final currentWords = this.mnemonicWords.value;

    if (currentWords.length >= amountWords) return;
    if (!this.isValidWord(word)) return;

    this.mnemonicWords.value = [...currentWords, word];
    this.textController.clear();
    suggestions.value = [];
    errorMessage.value = null;
    textFocusNode.requestFocus();
  }

  void onRemoveLastWord() {
    final currentWords = this.mnemonicWords.value;
    if (currentWords.isEmpty) return;
    mnemonicWords.value = currentWords.sublist(0, currentWords.length - 1);
    textFocusNode.requestFocus();
  }

  bool isValidWord(String word) {
    return form.isMnemonicWord(word: word);
  }

  Future<void> onSubmit() async {
    if (this.isRestoring.value) return;

    this.isRestoring.value = true;
    try {
      await this.onSubmitInner();
    } finally {
      if (this.mounted) this.isRestoring.value = false;
    }
  }

  Future<void> onSubmitInner() async {
    info("restore: user restores from seed");
    final wordList = this.mnemonicWords.value;
    final restoreApi = this.widget.restoreApi;
    final config = this.widget.config;
    final rootSeedResult = Result.tryFfi(
      () => RootSeed.fromMnemonic(mnemonic: wordList),
    );
    final RootSeed rootSeed;
    switch (rootSeedResult) {
      case Ok(:final ok):
        rootSeed = ok;
      case Err(:final err):
        this.errorMessage.value = ErrorMessage(
          title: "Error restoring wallet",
          message: err.message,
        );
        return;
    }

    final result = await restoreApi.restore(
      config: config,
      googleAuthCode: null,
      rootSeed: rootSeed,
    );
    if (!this.mounted) return;
    final AppHandle flowResult;
    switch (result) {
      case Ok(:final ok):
        flowResult = ok;
      case Err(:final err):
        error("restore: AppHandle.restore failed: $err");
        this.errorMessage.value = ErrorMessage(
          title: "Error restoring wallet",
          message: err.message,
        );
        return;
    }
    unawaited(Navigator.of(this.context).maybePop(flowResult));
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
        actions: const [
          LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
          SizedBox(width: Space.s400),
        ],
      ),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "Enter your seed phrase"),
          const SizedBox(height: Space.s200),
          const SubheadingText(
            text: "Your recovery phrase is a list of 24 words.",
          ),
          const SizedBox(height: Space.s600),
          TextField(
            controller: this.textController,
            focusNode: this.textFocusNode,
            onChanged: this.onTextChanged,
            decoration: baseInputDecoration.copyWith(hintText: "Enter word"),
            autocorrect: false,
            enableSuggestions: false,
            textInputAction: TextInputAction.done,
          ),
          ValueListenableBuilder(
            valueListenable: this.suggestions,
            builder: (context, suggestions, widget) => WordSuggestionsRow(
              suggestions: this.suggestions,
              onWordTap: this.onWordSelected,
            ),
          ),
          const SizedBox(height: Space.s200),
          ValueListenableBuilder(
            valueListenable: this.mnemonicWords,
            builder: (context, mnemonicWords, widget) => Align(
              alignment: Alignment.center,
              child: SeedWordsCard.removable(
                seedWords: this.mnemonicWords.value,
                onRemove: this.onRemoveLastWord,
              ),
            ),
          ),
          const SizedBox(height: Space.s200),
          ValueListenableBuilder(
            valueListenable: this.errorMessage,
            builder: (context, errorMessage, _) =>
                ErrorMessageSection(errorMessage),
          ),
        ],
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: ValueListenableBuilder(
            valueListenable: this.mnemonicWords,
            builder: (context, value, child) {
              return ValueListenableBuilder(
                valueListenable: this.isRestoring,
                builder: (context, isRestoring, widget) => AnimatedFillButton(
                  onTap: this.mnemonicWords.value.length >= amountWords
                      ? this.onSubmit
                      : null,
                  loading: isRestoring,
                  label: const Text("Restore"),
                  icon: const Icon(LxIcons.next),
                  style: FilledButton.styleFrom(
                    backgroundColor: LxColors.moneyGoUp,
                    foregroundColor: LxColors.grey1000,
                    iconColor: LxColors.grey1000,
                  ),
                ),
              );
            },
          ),
        ),
      ),
    );
  }
}

class WordSuggestionsRow extends StatelessWidget {
  const WordSuggestionsRow({
    super.key,
    required this.suggestions,
    required this.onWordTap,
  });

  final ValueNotifier<List<String>> suggestions;
  final ValueChanged<String> onWordTap;

  @override
  Widget build(BuildContext context) {
    final suggestions = this.suggestions.value;
    if (suggestions.isEmpty) return const SizedBox(height: Space.s800);

    return Container(
      padding: const EdgeInsets.symmetric(vertical: Space.s200),
      height: Space.s800,
      child: ListView.separated(
        scrollDirection: Axis.horizontal,
        itemCount: suggestions.length,
        separatorBuilder: (context, index) => const SizedBox(width: Space.s100),
        itemBuilder: (context, index) {
          final word = suggestions[index];
          return SuggestionChip(word: word, onTap: () => onWordTap(word));
        },
      ),
    );
  }
}

class SuggestionChip extends StatelessWidget {
  const SuggestionChip({super.key, required this.word, required this.onTap});

  final String word;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: this.onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(
          horizontal: Space.s400,
          vertical: Space.s100,
        ),
        child: Text(
          this.word,
          style: const TextStyle(
            fontSize: Fonts.size200,
            fontVariations: [Fonts.weightMedium],
            color: LxColors.linkText,
          ),
        ),
      ),
    );
  }
}
