import 'package:flutter/material.dart';
import 'package:google_sign_in/google_sign_in.dart' show GoogleSignIn;
import 'package:lexeapp/logger.dart' show error;

import '../../components.dart'
    show HeadingText, LxBackButton, LxFilledButton, ScrollableSinglePageBody;
import '../../result.dart' show MessageException;
import '../../style.dart' show Space;

// This `GoogleSignIn` class tracks the currently signed-in Google user (if any)
// and let's us request Google oauth2 scopes (permissions). On iOS/macOS it
// opens Safari to a google consent screen, while on Android it uses a more
// integrated in-app chrome tab. Doesn't support other platforms (Linux/Windows).
// We'll need more extensive engineering to add support for them.
//
// A global singleton. Using multiple instances won't help, since they all share
// the same underlying state under the ffi layer.
final _googleSignIn = GoogleSignIn(
  // The GDrive permissions we need.
  //
  // > Create new Drive files, or modify existing files, that you open with an
  // > app or that the user shares with an app while using the Google Picker API
  // > or the app's file picker.
  // >
  // > (Not-sensitive)
  //
  // <https://developers.google.com/drive/api/guides/api-specific-auth>
  scopes: ["https://www.googleapis.com/auth/drive.file"],
);

/// After successfully getting user consent
@immutable
final class GDriveAuthInfo {
  const GDriveAuthInfo({required this.authCode});

  final String authCode;
}

/// Open a browser window to request user consent for GDrive file permissions.
///
/// - On success, returns a [GDriveAuthInfo], which contains an auth code that
///   the backend user enclave can use to get real access+refresh tokens.
/// - If the user cancels, returns null.
/// - May throw an [Exception].
///
/// This flow lets us access GDrive from the remote enclave while doing the
/// authn+authz in the mobile app. Each platform is configured with a
/// "serverClientId", which is the oauth client id used by our backend node
/// enclaves. This id is different from the "clientId"; in fact, each client
/// platform actually has its own separate clientId.
///
/// The `serverAuthCode` we get here on the client is a one-time use
/// auth code that we can exchange on the node enclave for an access+refresh
/// token. We use those tokens inside the enclave to access GDrive on behalf of
/// the user.
///
/// Docs: <https://developers.google.com/identity/sign-in/ios/offline-access>
/// Android config: <../../android/app/src/main/res/values/strings.xml>
/// iOS config: <../../ios/Runner/Info.plist>
/// macOS config: <../../macos/Runner/Info.plist>
Future<GDriveAuthInfo?> tryGDriveAuth() async {
  final account = await _googleSignIn.signIn();
  // Can be null if user canceled sign, rejected consent, hit back button, etc...
  if (account == null) return null;

  final serverAuthCode = account.serverAuthCode;
  if (serverAuthCode == null) {
    // This only seems to happen when our app is misconfigured
    throw const MessageException("response is missing serverAuthCode");
  }

  return GDriveAuthInfo(authCode: serverAuthCode);
}
