/// Ask the user for Google Drive authorization via OAuth2.
///
/// We previously used the `google_sign_in` flutter package, but later switched
/// to the more generic `flutter_web_auth_2` package for greater control. The
/// main issue is that `google_sign_in` mandates additional scopes, namely
/// `openid`, `https://www.googleapis.com/auth/userinfo.email` and
/// `https://www.googleapis.com/auth/userinfo.profile`.
library;

import 'dart:convert' show jsonDecode;
import 'dart:io' show Platform;

import 'package:app_rs_dart/ffi/types.dart' show DeployEnv;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show PlatformException, appFlavor;
import 'package:flutter_web_auth_2/flutter_web_auth_2.dart'
    show FlutterWebAuth2;
import 'package:http/http.dart' as http;
import 'package:lexeapp/cfg.dart' as cfg;
import 'package:lexeapp/result.dart';

// The GDrive permissions we need.
//
// > Create new Drive files, or modify existing files, that you open with an
// > app or that the user shares with an app while using the Google Picker API
// > or the app's file picker.
// >
// > (Not-sensitive)
//
// <https://developers.google.com/drive/api/guides/api-specific-auth>
const String gdriveScope = "https://www.googleapis.com/auth/drive.file";

/// Returned from Google OAuth2 after getting user consent.
@immutable
final class GDriveAuthInfo {
  const GDriveAuthInfo({required this.serverAuthCode});

  final String serverAuthCode;
}

abstract interface class GDriveAuth {
  static const GDriveAuth prod = ProdGDriveAuth._();
  static const GDriveAuth mock = MockGDriveAuth._();

  /// Open a browser window to request user consent for GDrive file permissions.
  ///
  /// - On success, returns a [GDriveAuthInfo], which contains an auth code that
  ///   the backend user enclave can use to get real access+refresh tokens.
  /// - If the user cancels, returns null.
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
  Future<Result<GDriveAuthInfo?, Exception>> tryAuth();
}

class ProdGDriveAuth implements GDriveAuth {
  const ProdGDriveAuth._();

  @override
  Future<Result<GDriveAuthInfo?, Exception>> tryAuth() async {
    final credentials = _GDriveCredentials._forPlatform();
    if (credentials == null) {
      final platform = Platform.operatingSystem;
      const flavor = appFlavor ?? "default";
      return Err(Exception(
          "Missing google drive credentials for ($platform, $flavor)"));
    }

    final redirectUriStr = credentials.redirectUri.toString();

    // TODO(phlip9): segment server credentials by deploy env?
    const serverClientId =
        "495704988639-19bfg8k5f3runiio4apbicpounc10gh1.apps.googleusercontent.com";

    // Keep this URL in sync with `gdrive::oauth2::test::test_auth_code_for_token`.
    final Map<String, String> queryParams = {
      "client_id": credentials.clientId,
      "redirect_uri": redirectUriStr,
      "response_type": "code",
      "scope": gdriveScope,
      "access_type": "offline",
      // Include this field so we also receive the server auth code when we
      // exchange for a token below. This server auth code is what the enclave
      // uses to exchange for its auth token and long-term refresh token.
      "audience": serverClientId,
    };
    final url =
        Uri.https("accounts.google.com", "/o/oauth2/v2/auth", queryParams);

    try {
      // Open a browser at `url` and wait for the user to authorize, which will
      // redirect them to `redirectUri`. `flutter_web_auth_2` registers a handler
      // for this URI (technically any URI with `callbackUriScheme` as the scheme)
      // and completes this future with the final redirect URI, which contains our
      // client auth code in its query parameters.
      final resultUriStr = await FlutterWebAuth2.authenticate(
        url: url.toString(),
        callbackUrlScheme: credentials.redirectUri.scheme,
      );

      // TODO(phlip9): move the rest of this flow to Rust
      // debug("auth result uri: '$resultUriStr'");

      // Extract the client auth code from the redirect URI
      final resultUri = Uri.parse(resultUriStr);
      final clientAuthCode = resultUri.queryParameters["code"];
      if (clientAuthCode == null) {
        return Err(Exception(
            "OAuth2 redirect is missing the client auth code: '$resultUriStr'"));
      }

      // Exchange the client auth code for an auth token and refresh token,
      // though we don't actually need them. Instead we just want the server
      // auth code.
      final accessTokenUrl = Uri.https("oauth2.googleapis.com", "/token");
      final response = await http.post(accessTokenUrl, body: {
        "client_id": credentials.clientId,
        "redirect_uri": redirectUriStr,
        "grant_type": "authorization_code",
        "code": clientAuthCode,
      });

      // Example `response.body`:
      //
      // ```json
      // {
      //   "access_token": "XXX",
      //   "expires_in": 3599,
      //   "refresh_token": "YYY",
      //   "scope": "https://www.googleapis.com/auth/drive.file",
      //   "token_type": "Bearer",
      //   "server_code": "ZZZ"
      // }
      // ```
      final responseJson = jsonDecode(response.body);
      if (responseJson["error"] != null) {
        final String errorKind = responseJson["error"] ?? "unknown";
        final String errorDescription = responseJson["error_description"] ?? "";
        return Err(Exception("$errorKind error: $errorDescription"));
      }

      final serverAuthCode = responseJson["server_code"];
      if (serverAuthCode == null) {
        // Redact secrets
        responseJson["access_token"] = "XXX";
        responseJson["refresh_token"] = "XXX";
        return Err(Exception(
            "Missing server_code in token response: '$responseJson'"));
      }

      return Ok(GDriveAuthInfo(serverAuthCode: serverAuthCode));
    } on PlatformException catch (err) {
      if (err.code == "CANCELED") {
        return const Ok(null);
      } else {
        return Err(err);
      }
    } on Exception catch (err) {
      return Err(err);
    }
  }
}

/// A basic mock [GDriveAuth] impl. It just returns a dummy auth token after a
/// delay, without doing any oauth.
class MockGDriveAuth implements GDriveAuth {
  const MockGDriveAuth._();

  @override
  Future<Result<GDriveAuthInfo?, Exception>> tryAuth() => Future.delayed(
        const Duration(milliseconds: 1200),
        () => const Ok(GDriveAuthInfo(serverAuthCode: "fake")),
        // () => Err(Exception(
        //     "PlatformException(sign_in_failed, com.google.android.gms.common.api.ApiException: 10: , null, null)")),
      );
}

/// A set of credentials for a particular platform to run the OAuth2 flow.
final class _GDriveCredentials {
  const _GDriveCredentials({
    required this.clientId,
    required this.redirectUri,
  });

  final String clientId;
  final Uri redirectUri;

  static _GDriveCredentials? _forPlatform() {
    final clientId = _clientId();
    if (clientId == null) {
      return null;
    }

    return _GDriveCredentials(
      clientId: clientId,
      redirectUri:
          Uri(scheme: _callbackSchemeFromClientId(clientId), path: "/"),
    );
  }

  /// Get the google oauth2 client_id for the current platform/deployEnv.
  static String? _clientId() {
    if (Platform.isIOS || Platform.isMacOS) {
      if (cfg.design) {
        return "495704988639-2rqsnvobrvlnbkqdin38q2r3cph537l5.apps.googleusercontent.com";
      } else {
        return switch (cfg.deployEnv) {
          DeployEnv.dev => null,
          DeployEnv.staging =>
            "495704988639-ook7rjckct44o668nt1f58sd3bharq2p.apps.googleusercontent.com",
          DeployEnv.prod => null,
        };
      }
    } else if (Platform.isAndroid) {
      // Keep these in sync with the values in `public/app/android/app/build.gradle`.
      if (cfg.design) {
        return "495704988639-qhjbk0nkfaibgr16h0gimlqcae8cl13e.apps.googleusercontent.com";
      } else {
        return switch (cfg.deployEnv) {
          DeployEnv.dev => null,
          DeployEnv.staging =>
            "495704988639-fvkq7thnksbqi7n3tanpopu5brr2pa4a.apps.googleusercontent.com",
          DeployEnv.prod => null,
        };
      }
    } else {
      // TODO(phlip9): support Linux & Windows desktop. I tried with Desktop
      // credentials, but the token exchange refused to return a `server_code`..
      return null;
    }
  }

  /// Build the callback URI scheme from the client_id. This is just the
  /// client_id but with reversed domain segments.
  static String _callbackSchemeFromClientId(final String clientId) =>
      clientId.split(".").reversed.join(".");
}
