/// Ask the user for Google Drive authorization via OAuth2.
///
/// We previously used the `google_sign_in` flutter package, but later switched
/// to the more generic `flutter_web_auth_2` package for greater control. The
/// main issue is that `google_sign_in` mandates additional scopes, namely
/// `openid`, `https://www.googleapis.com/auth/userinfo.email` and
/// `https://www.googleapis.com/auth/userinfo.profile`.
library;

import 'dart:io' show Platform;

import 'package:app_rs_dart/ffi/gdrive.dart'
    show
        GDriveClient,
        GDriveClientInner,
        GDriveOAuth2Flow,
        GDriveRestoreCandidate,
        GDriveRestoreCandidateRs,
        GDriveRestoreClient,
        GDriveRestoreClientRs;
import 'package:app_rs_dart/ffi/types.dart' show DeployEnv, Network, RootSeed;
import 'package:app_rs_dart/frb.dart' show AnyhowException;
import 'package:app_rs_dart/lib.dart' show RootSeedRs;
import 'package:flutter/services.dart' show PlatformException, appFlavor;
import 'package:flutter_web_auth_2/flutter_web_auth_2.dart'
    show FlutterWebAuth2;
import 'package:lexeapp/cfg.dart' as cfg;
import 'package:lexeapp/result.dart';

/// Returned from Google OAuth2 after getting user consent. Sent to the node
/// enclave during initial provisioning.
final class GDriveServerAuthCode {
  const GDriveServerAuthCode({required this.serverAuthCode});

  final String serverAuthCode;
}

abstract class GDriveAuth {
  const GDriveAuth._();

  static const GDriveAuth prod = ProdGDriveAuth._();
  static const GDriveAuth mock = MockGDriveAuth._();
  static const GDriveAuth mockError = MockErrorGDriveAuth._();

  /// Open a browser window to request user consent for GDrive file permissions.
  ///
  /// - On success, returns a [GDriveClient], which contains an auth code that
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
  Future<Result<GDriveClient?, Exception>> tryAuth();

  /// [tryAuth] but we only care about the `serverCode`.
  Future<Result<GDriveServerAuthCode?, Exception>> tryAuthCodeOnly() async {
    switch (await this.tryAuth()) {
      case Ok(:final ok):
        // User canceled
        if (ok == null) return const Ok(null);
        final serverAuthCode = ok.serverCode();
        if (serverAuthCode == null) {
          return Err(Exception("GDrive auth didn't return a server auth code"));
        }
        return Ok(GDriveServerAuthCode(serverAuthCode: serverAuthCode));
      case Err(:final err):
        return Err(err);
    }
  }
}

class ProdGDriveAuth extends GDriveAuth {
  const ProdGDriveAuth._() : super._();

  @override
  Future<Result<GDriveClient?, Exception>> tryAuth() async {
    final clientId = _clientId();
    if (clientId == null) {
      final platform = Platform.operatingSystem;
      const flavor = appFlavor ?? "default";
      return Err(
        Exception("Missing google drive client id for ($platform, $flavor)"),
      );
    }

    // TODO(phlip9): segment server credentials by deploy env?
    const serverClientId =
        "495704988639-19bfg8k5f3runiio4apbicpounc10gh1.apps.googleusercontent.com";

    final oauthFlow = GDriveOAuth2Flow.init(
      clientId: clientId,
      serverClientId: serverClientId,
    );

    // // Uncomment while debugging
    // info("oauth2 flow init:");
    // info("  client_id: ${oauthFlow.clientId}");
    // info("  code_verifier: ${oauthFlow.codeVerifier}");
    // info("  redirect_uri: ${oauthFlow.redirectUri}");
    // info("  redirect_uri_scheme: ${oauthFlow.redirectUriScheme}");
    // info("  url: ${oauthFlow.url}");

    try {
      // Open a browser at `url` and wait for the user to authorize, which will
      // redirect them to `redirectUri`. `flutter_web_auth_2` registers a handler
      // for this URI (technically any URI with `callbackUriScheme` as the scheme)
      // and completes this future with the final redirect URI, which contains our
      // client auth code in its query parameters.
      final resultUriStr = await FlutterWebAuth2.authenticate(
        url: oauthFlow.url,
        callbackUrlScheme: oauthFlow.redirectUriScheme,
      );

      // Exchange auth code from redirect for access token + server auth code.
      final gdriveClient = await oauthFlow.exchange(resultUri: resultUriStr);

      return Ok(gdriveClient);
    } on PlatformException catch (err) {
      if (err.code == "CANCELED") {
        return const Ok(null);
      } else {
        return Err(err);
      }
    } on AnyhowException catch (err) {
      return Err(FfiError.fromFfi(err));
    } on Exception catch (err) {
      return Err(err);
    }
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
          DeployEnv.prod =>
            "495704988639-mh0ja8lln9q5ru2kshltpsqbb7ns2bru.apps.googleusercontent.com",
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
          DeployEnv.prod =>
            "495704988639-cr7bvcr117n7aks3p3e3qntoa7ps0lj1.apps.googleusercontent.com",
        };
      }
    } else {
      // TODO(phlip9): support Linux & Windows desktop. I tried with Desktop
      // credentials, but the token exchange refused to return a `server_code`..
      return null;
    }
  }
}

/// A basic mock [GDriveAuth] impl. It just returns a dummy auth token after a
/// delay, without doing any oauth.
class MockGDriveAuth extends GDriveAuth {
  const MockGDriveAuth._() : super._();

  @override
  Future<Result<GDriveClient?, Exception>> tryAuth() => Future.delayed(
    const Duration(milliseconds: 1200),
    () => const Ok(MockGDriveClient._()),
  );
}

/// A basic mock [GDriveAuth] impl. It unconditionally returns an error message
/// after a delay.
class MockErrorGDriveAuth extends GDriveAuth {
  const MockErrorGDriveAuth._() : super._();

  @override
  Future<Result<GDriveClient?, Exception>> tryAuth() => Future.delayed(
    const Duration(milliseconds: 1200),
    () => const Err(
      FfiError(
        "Auth code exchange failed\n\nCaused by:\n  1. stacktrace error gets cut off\n  1. stacktrace error gets cut off\n  1. stacktrace error gets cut off\n  1. stacktrace error gets cut off\n  1. stacktrace error gets cut off\n  1. stacktrace error gets cut off\n  1. stacktrace error gets cut off\n  1. stacktrace error gets cut off\n  1. stacktrace error gets cut off\n  1. stacktrace error gets cut off\n  1. stacktrace error gets cut off",
      ),
    ),
  );
}

class MockGDriveClient implements GDriveClient {
  const MockGDriveClient._();

  @override
  GDriveClientInner get inner => throw UnimplementedError();

  @override
  String? serverCode() => "fake";

  @override
  GDriveRestoreClient intoRestoreClient() => const MockGDriveRestoreClient._();

  @override
  Future<String> dumpState({
    required DeployEnv deployEnv,
    required Network network,
    required bool useSgx,
    required RootSeed rootSeed,
  }) => Future.value('{"channel_manager": null, channel_monitors: null}');
}

class MockGDriveRestoreClient implements GDriveRestoreClient {
  const MockGDriveRestoreClient._();

  @override
  Future<List<GDriveRestoreCandidate>> findRestoreCandidates({
    required DeployEnv deployEnv,
    required Network network,
    required bool useSgx,
  }) => Future.delayed(
    const Duration(milliseconds: 1234),
    () => [
      const MockGDriveRestoreCandidate(
        userPk:
            "4072836db6c62f1fd07281feb1f2d6d1b8f05f8be3f0019a9205edff244017f1",
      ),
      const MockGDriveRestoreCandidate(
        userPk:
            "ef64652cc9fc1d79d174bb52d0ffb7ad365db842e72e056aa5c4bfe00bcb20da",
      ),
    ],
  );

  @override
  GDriveRestoreClientRs get inner => throw UnimplementedError();
}

class MockGDriveRestoreCandidate implements GDriveRestoreCandidate {
  const MockGDriveRestoreCandidate({required String userPk}) : _userPk = userPk;

  final String _userPk;

  @override
  GDriveRestoreCandidateRs get inner => throw UnimplementedError();

  @override
  RootSeed tryDecrypt({required String password}) =>
      // throw AnyhowException("Password decryption failed: ciphertext malformed");
      const MockRootSeed._();

  @override
  String userPk() => this._userPk;
}

class MockRootSeed implements RootSeed {
  const MockRootSeed._();

  @override
  RootSeedRs get inner => throw UnimplementedError();

  @override
  String exposeSecretHex() =>
      "5c2aa5fdd678112c8b13d745b5c1d1e1a81ace76721ec72f1424bd2eb387a8af";
}
