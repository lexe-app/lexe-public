import 'package:flutter/material.dart';
import 'package:google_sign_in/google_sign_in.dart'
    show GoogleSignIn, GoogleSignInAccount;

import 'package:lexeapp/result.dart';

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

/// Returned from Google OAuth 2 after getting user consent.
@immutable
final class GDriveAuthInfo {
  const GDriveAuthInfo({required this.authCode});

  final String authCode;
}

abstract interface class GDriveAuth {
  static const GDriveAuth prod = ProdGDriveAuth();
  static const GDriveAuth mock = MockGDriveAuth();

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
  /// token. We use those tokens inside the enclave to access GDrive on behalf
  /// of the user.
  ///
  /// Docs: <https://developers.google.com/identity/sign-in/ios/offline-access>
  /// Android config: <../../android/app/src/main/res/values/strings.xml>
  /// iOS config: <../../ios/Runner/Info.plist>
  /// macOS config: <../../macos/Runner/Info.plist>
  Future<Result<GDriveAuthInfo?, Exception>> tryAuth();
}

class ProdGDriveAuth implements GDriveAuth {
  const ProdGDriveAuth();

  @override
  Future<Result<GDriveAuthInfo?, Exception>> tryAuth() async {
    final result = await Result.tryAsync<GoogleSignInAccount?, Exception>(
        _googleSignIn.signIn);

    final String? serverAuthCode;
    switch (result) {
      case Ok(:final ok):
        // Can be null if user canceled sign, rejected consent, hit back button
        if (ok == null) return const Ok(null);
        serverAuthCode = ok.serverAuthCode;
      case Err(:final err):
        return Err(err);
    }

    if (serverAuthCode == null) {
      // This should only happen when our app is misconfigured (missing
      // serverClientId configuration).
      throw StateError("response is missing serverAuthCode");
    }

    return Ok(GDriveAuthInfo(authCode: serverAuthCode));
  }
}

/// A basic mock [GDriveAuth] impl. It just returns a dummy auth token after a
/// delay, without doing any oauth.
class MockGDriveAuth implements GDriveAuth {
  const MockGDriveAuth();

  @override
  Future<Result<GDriveAuthInfo?, Exception>> tryAuth() => Future.delayed(
        const Duration(milliseconds: 1200),
        () => const Ok(GDriveAuthInfo(authCode: "fake")),
      );
}
