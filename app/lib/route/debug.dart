/// # Lexe Debug Page
///
/// A page for manipulating app internals during development.
library;

import 'dart:convert' show utf8;

import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/debug.dart' as debug;
import 'package:app_rs_dart/ffi/secret_store.dart' show SecretStore;
import 'package:app_rs_dart/ffi/types.dart' show Config;
import 'package:flutter/material.dart';
import 'package:lexeapp/clipboard.dart' show LxClipboard;
import 'package:lexeapp/components.dart'
    show
        HeadingText,
        LxCloseButton,
        ScrollableSinglePageBody,
        SubheadingText,
        showModalAsyncFlow;
import 'package:lexeapp/gdrive_auth.dart' show GDriveAuth;
import 'package:lexeapp/logger.dart' show error, info;
import 'package:lexeapp/result.dart';
import 'package:lexeapp/save_file.dart' as save_file;
import 'package:lexeapp/settings.dart' show LxSettings;
import 'package:lexeapp/style.dart' show LxColors, Space;

class DebugPage extends StatefulWidget {
  const DebugPage({
    super.key,
    required this.config,
    required this.app,
    required this.settings,
    required this.gdriveAuth,
  });

  final Config config;
  final AppHandle app;
  final LxSettings settings;
  final GDriveAuth gdriveAuth;

  @override
  State<DebugPage> createState() => _DebugPageState();
}

class _DebugPageState extends State<DebugPage> {
  Future<void> doDeleteLocalPaymentDb() async {
    info("Deleting local PaymentDb");

    (await Result.tryFfiAsync(
      this.widget.app.deletePaymentDb,
    )).inspectErr((err) => error(err.message));
  }

  void doDeleteSecretStore() {
    info("Deleting SecretStore");

    Result.tryFfi(
      () => debug.deleteSecretStore(config: this.widget.config),
    ).inspectErr((err) => error(err.message));
  }

  // void doDeleteLatestProvisionedFile() {

  void doResetSettingsDb() {
    info("Resetting SettingsDb");
    this.widget.settings.reset();
  }

  Future<void> copyRootSeed() async {
    final secretStore = SecretStore(config: this.widget.config);
    final rootSeed = secretStore.readRootSeed();
    if (rootSeed == null) return;

    return LxClipboard.copyTextWithFeedback(
      this.context,
      rootSeed.exposeSecretHex(),
    );
  }

  /// Re-auth the user with GDrive and then dump the persisted node state
  /// (channel manager and channel monitors) to a JSON file.
  Future<void> dumpNodeStateFromGdrive() async {
    await showModalAsyncFlow(
      context: this.context,
      future: Result.tryAsync<void, Exception>(
        this.dumpNodeStateFromGdriveInner,
      ),
      errorBuilder: (context, err) => AlertDialog(
        title: const Text("Issue dumping state"),
        content: Text(err.toString()),
        scrollable: true,
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text("Close"),
          ),
        ],
      ),
      barrierDismissible: true,
    );
  }

  Future<void> dumpNodeStateFromGdriveInner() async {
    // TODO(phlip9): pass thru `SecretStore` from app load/restore
    final config = this.widget.config;
    final secretStore = SecretStore(config: config);

    // Read user RootSeed from local secret store.
    final rootSeed = secretStore.readRootSeed();
    if (rootSeed == null) {
      throw Exception("Missing root seed in local device secret store");
    }

    // Ask user to auth with Google Drive.
    final gdriveClient = (await this.widget.gdriveAuth.tryAuth()).unwrap();
    // User canceled
    if (!this.mounted || gdriveClient == null) return;

    // Dump the node state from GDrive.
    final nodeStateJson = await gdriveClient.dumpState(
      deployEnv: config.deployEnv,
      network: config.network,
      useSgx: config.useSgx,
      rootSeed: rootSeed,
    );
    if (!this.mounted) return;

    // Ask user to save the node state JSON to a file.
    await save_file.openDialog(
      filename: "node_state.json",
      data: utf8.encode(nodeStateJson),
    );
  }

  @override
  Widget build(BuildContext context) {
    const bodyPadding = EdgeInsets.symmetric(horizontal: Space.s600);

    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxCloseButton(isLeading: true),
      ),
      body: ScrollableSinglePageBody(
        padding: bodyPadding,
        body: [
          const HeadingText(text: "Lexe Debug"),
          const SubheadingText(text: "Page for manipulating app internals."),
          const SizedBox(height: Space.s600),

          // Reset SettingsDb
          ListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text("Reset settings"),
            subtitle: const Text.rich(
              TextSpan(
                children: [
                  TextSpan(
                    text: "Resets all settings to their default values.",
                    style: TextStyle(color: LxColors.fgTertiary),
                  ),
                ],
              ),
            ),
            onTap: this.doResetSettingsDb,
          ),

          // Delete PaymentDb
          ListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text("Delete local payments"),
            subtitle: const Text(
              "Your app will clear all local payment info and resync from the node",
              style: TextStyle(color: LxColors.fgTertiary),
            ),
            onTap: this.doDeleteLocalPaymentDb,
          ),

          // // TODO(phlip9): actually delete latest_provisioned
          // ListTile(
          //   contentPadding: EdgeInsets.zero,
          //   title: const Text("Delete latest_provisioned file (TODO)"),
          //   subtitle: const Text(
          //     "On next restart, will ask the Lexe API for the most recent node "
          //     "version and unconditionally provision to it.",
          //     style: TextStyle(color: LxColors.fgTertiary),
          //   ),
          //   onTap: this.doDeleteLatestProvisionedFile,
          // ),

          // Delete SecretStore
          ListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text("Delete local secrets"),
            subtitle: const Text.rich(
              TextSpan(
                children: [
                  TextSpan(
                    text: "WARNING: ",
                    style: TextStyle(color: Color(0xffeb5d47)),
                  ),
                  TextSpan(
                    text:
                        "you will need to recover from backup to use this wallet again",
                    style: TextStyle(color: LxColors.fgTertiary),
                  ),
                ],
              ),
            ),
            onTap: this.doDeleteSecretStore,
          ),

          // Copy RootSeed to clipboard
          ListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text("Copy RootSeed to clipboard"),
            subtitle: const Text.rich(
              TextSpan(
                children: [
                  TextSpan(
                    text: "WARNING: ",
                    style: TextStyle(color: Color(0xffeb5d47)),
                  ),
                  TextSpan(
                    text:
                        "this is the root seed for your wallet. Anyone "
                        "with this secret also controls your funds.",
                    style: TextStyle(color: LxColors.fgTertiary),
                  ),
                ],
              ),
            ),
            onTap: this.copyRootSeed,
          ),

          // Dump node state from GDrive
          ListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text("Dump node state from GDrive"),
            subtitle: const Text.rich(
              TextSpan(
                children: [
                  TextSpan(
                    text: "WARNING: ",
                    style: TextStyle(color: Color(0xffeb5d47)),
                  ),
                  TextSpan(
                    text:
                        "this contains some secrets and sensitive information about your node. Only share this with someone you trust.",
                    style: TextStyle(color: LxColors.fgTertiary),
                  ),
                ],
              ),
            ),
            onTap: this.dumpNodeStateFromGdrive,
          ),
        ],
      ),
    );
  }
}
