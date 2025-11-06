import 'dart:async';

import 'package:app_rs_dart/ffi/types.dart' show Username;
import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/material.dart';
import 'package:lexeapp/components.dart'
    show
        ErrorMessage,
        ErrorMessageSection,
        HeadingText,
        LxBackButton,
        LxCloseButton,
        LxCloseButtonKind,
        LxFilledButton,
        MultistepFlow,
        ScrollableSinglePageBody,
        SubheadingText,
        baseInputDecoration;
import 'package:lexeapp/logger.dart' show info;
import 'package:lexeapp/result.dart';
import 'package:lexeapp/service/payment_address.dart'
    show PaymentAddressService;
import 'package:lexeapp/style.dart' show Fonts, LxColors, LxIcons, Space;

/// The entry point for the profile flow.
class ProfilePage extends StatelessWidget {
  const ProfilePage({super.key, required this.paymentAddressService});

  final PaymentAddressService paymentAddressService;

  @override
  Widget build(BuildContext context) => MultistepFlow<String?>(
    builder: (_) => EditPaymentAddressPage(
      paymentAddressService: this.paymentAddressService,
    ),
  );
}

/// Page to edit/set the user's payment address (username@lexe.app).
class EditPaymentAddressPage extends StatefulWidget {
  const EditPaymentAddressPage({
    super.key,
    required this.paymentAddressService,
  });

  final PaymentAddressService paymentAddressService;

  @override
  State<EditPaymentAddressPage> createState() => _EditPaymentAddressPageState();
}

class _EditPaymentAddressPageState extends State<EditPaymentAddressPage> {
  final GlobalKey<FormFieldState<String>> usernameKey = GlobalKey();
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  ValueListenable<bool> get isLoading =>
      this.widget.paymentAddressService.isUpdating;

  bool get isUpdatable =>
      this.widget.paymentAddressService.paymentAddress.value?.updatable == true;

  String? get initialUsername =>
      this.widget.paymentAddressService.paymentAddress.value?.username?.field0;

  @override
  void initState() {
    this.widget.paymentAddressService.fetch();
    super.initState();
  }

  @override
  void dispose() {
    this.errorMessage.dispose();
    super.dispose();
  }

  Result<Username, String?> validateUsername(final String? username) {
    if (username == null || username.isEmpty) {
      return const Err("Username is required");
    }

    final trimmed = username.trim();

    // Username type enfoces 1 lenth minimum. Here we force it to be at least 4 characters.
    if (trimmed.length < 4) {
      return const Err("Username must be at least 4 characters");
    }

    final result = Result.tryFfi(() => Username.parse(s: trimmed));
    switch (result) {
      case Ok(:final ok):
        return Ok(ok);
      case Err(:final err):
        return Err("$err");
    }
  }

  Future<void> onSubmit() async {
    if (this.widget.paymentAddressService.isDisposed) return;
    if (this.isLoading.value) return;
    if (!this.isUpdatable) {
      this.errorMessage.value = const ErrorMessage(
        title: "Error",
        message: "Payment address is not updatable. Please try later.",
      );
      return;
    }

    final usernameField = this.usernameKey.currentState!;
    if (!usernameField.validate()) {
      return;
    }

    final Username username;
    switch (this.validateUsername(usernameField.value)) {
      case Ok(:final ok):
        username = ok;
      case Err():
        return;
    }

    // Clear error message
    this.errorMessage.value = null;

    info("EditPaymentAddressPage: updating username to ${username.field0}");
    final res = await this.widget.paymentAddressService.update(
      username: username,
    );
    if (!this.mounted) return;
    if (res.isErr) {
      this.errorMessage.value = ErrorMessage(title: "Error", message: res.err);
      return;
    }

    await Navigator.of(this.context).pushReplacement(
      MaterialPageRoute(
        builder: (context) =>
            PaymentAddressSuccessPage(username: username.field0),
      ),
    );
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
          const HeadingText(text: "Update your username"),
          const SubheadingText(
            text:
                "Receive money into your wallet instantly with only your ₿itcoin payment address.",
          ),
          const SizedBox(height: Space.s600),

          // Username field with @lexe.app suffix
          TextFormField(
            key: this.usernameKey,
            autofocus: true,
            initialValue: this.initialUsername,
            textInputAction: TextInputAction.done,
            validator: (str) => this.validateUsername(str).err,
            onEditingComplete: this.onSubmit,
            decoration: baseInputDecoration.copyWith(
              hintText: "username",
              prefixText: "₿",
              suffixText: "@lexe.app",
              suffixStyle: Fonts.fontUI.copyWith(
                fontSize: Fonts.size700,
                color: LxColors.grey600,
                fontVariations: [Fonts.weightMedium],
              ),
              prefixStyle: Fonts.fontUI.copyWith(
                fontSize: Fonts.size700,
                color: LxColors.grey600,
                fontVariations: [Fonts.weightMedium],
              ),
              errorMaxLines: 2,
            ),
            obscureText: false,
            enableSuggestions: false,
            autocorrect: false,
            style: Fonts.fontUI.copyWith(
              fontSize: Fonts.size700,
              fontVariations: [Fonts.weightMedium],
              letterSpacing: -0.5,
            ),
          ),
          const SizedBox(height: Space.s500),

          // Error message section
          ValueListenableBuilder(
            valueListenable: this.errorMessage,
            builder: (_context, errorMessage, _widget) =>
                ErrorMessageSection(errorMessage),
          ),
        ],
        bottom: ValueListenableBuilder(
          valueListenable: this.isLoading,
          builder: (_context, isLoading, _widget) => LxFilledButton.strong(
            label: const Text("Continue"),
            icon: const Icon(LxIcons.next),
            onTap: isLoading ? null : this.onSubmit,
          ),
        ),
      ),
    );
  }
}

/// Success page shown after payment address is updated.
class PaymentAddressSuccessPage extends StatelessWidget {
  const PaymentAddressSuccessPage({super.key, required this.username});

  final String username;

  String get paymentAddress => "₿${this.username}@lexe.app";

  void onDone(BuildContext context) {
    Navigator.of(context, rootNavigator: true).pop(this.username);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        automaticallyImplyLeading: false,
        actions: const [
          LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
          SizedBox(width: Space.s400),
        ],
      ),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "Your new ₿itcoin address"),
          const SubheadingText(text: "Your username has been updated."),
          const SizedBox(height: Space.s500),

          Container(
            padding: const EdgeInsets.all(Space.s500),
            decoration: BoxDecoration(
              color: LxColors.grey950,
              borderRadius: BorderRadius.circular(12.0),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  "₿itcoin payment address",
                  style: Fonts.fontUI.copyWith(
                    fontSize: Fonts.size200,
                    color: LxColors.grey600,
                  ),
                ),
                const SizedBox(height: Space.s200),
                Text(
                  this.paymentAddress,
                  style: Fonts.fontUI.copyWith(
                    fontSize: Fonts.size500,
                    fontVariations: [Fonts.weightMedium],
                    color: LxColors.foreground,
                  ),
                ),
              ],
            ),
          ),
        ],
        bottom: LxFilledButton.strong(
          label: const Text("Done"),
          icon: const Icon(LxIcons.next),
          onTap: () => this.onDone(context),
        ),
      ),
    );
  }
}
