// This file is automatically generated, so please do not edit it.
// @generated by `flutter_rust_bridge`@ 2.7.1.

//
// From: `dart_preamble` in `app-rs-codegen/src/lib.rs`
// ignore_for_file: invalid_internal_annotation, always_use_package_imports, directives_ordering, prefer_const_constructors, sort_unnamed_constructors_first
//

// ignore_for_file: invalid_use_of_internal_member, unused_import, unnecessary_import

import '../frb_generated.dart';
import '../lib.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'types.dart';

// Rust type: RustOpaqueNom<GDriveClientInner>
abstract class GDriveClientInner implements RustOpaqueInterface {}

// Rust type: RustOpaqueNom<GDriveRestoreCandidateRs>
abstract class GDriveRestoreCandidateRs implements RustOpaqueInterface {}

// Rust type: RustOpaqueNom<GDriveRestoreClientRs>
abstract class GDriveRestoreClientRs implements RustOpaqueInterface {}

/// A basic authenticated Google Drive client, before we know which `UserPk`
/// to use.
class GDriveClient {
  final GDriveClientInner inner;

  const GDriveClient({required this.inner});

  /// Read the core persisted Node state from the user's Google Drive VFS
  /// and dump it as a JSON blob.
  ///
  /// Used for debugging.
  Future<String> dumpState({
    required DeployEnv deployEnv,
    required Network network,
    required bool useSgx,
    required RootSeed rootSeed,
  }) => AppRs.instance.api.crateFfiGdriveGDriveClientDumpState(
    that: this,
    deployEnv: deployEnv,
    network: network,
    useSgx: useSgx,
    rootSeed: rootSeed,
  );

  GDriveRestoreClient intoRestoreClient() => AppRs.instance.api
      .crateFfiGdriveGDriveClientIntoRestoreClient(that: this);

  String? serverCode() =>
      AppRs.instance.api.crateFfiGdriveGDriveClientServerCode(that: this);

  @override
  int get hashCode => inner.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is GDriveClient &&
          runtimeType == other.runtimeType &&
          inner == other.inner;
}

/// Context required to execute the Google Drive OAuth2 authorization flow.
class GDriveOAuth2Flow {
  final String clientId;
  final String codeVerifier;
  final String redirectUri;
  final String redirectUriScheme;
  final String url;

  const GDriveOAuth2Flow({
    required this.clientId,
    required this.codeVerifier,
    required this.redirectUri,
    required this.redirectUriScheme,
    required this.url,
  });

  /// After the user has authorized access and we've gotten the redirect,
  /// call this fn to exchange the client auth code for credentials + client.
  Future<GDriveClient> exchange({required String resultUri}) => AppRs
      .instance
      .api
      .crateFfiGdriveGDriveOAuth2FlowExchange(that: this, resultUri: resultUri);

  /// Begin the OAuth2 flow for the given mobile `client_id`. We'll also get
  /// a `server_code` we can exchange at the node provision enclave, which
  /// uses `server_client_id`.
  static GDriveOAuth2Flow init({
    required String clientId,
    required String serverClientId,
  }) => AppRs.instance.api.crateFfiGdriveGDriveOAuth2FlowInit(
    clientId: clientId,
    serverClientId: serverClientId,
  );

  @override
  int get hashCode =>
      clientId.hashCode ^
      codeVerifier.hashCode ^
      redirectUri.hashCode ^
      redirectUriScheme.hashCode ^
      url.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is GDriveOAuth2Flow &&
          runtimeType == other.runtimeType &&
          clientId == other.clientId &&
          codeVerifier == other.codeVerifier &&
          redirectUri == other.redirectUri &&
          redirectUriScheme == other.redirectUriScheme &&
          url == other.url;
}

/// A candidate root seed backup. We just need the correct password to restore.
class GDriveRestoreCandidate {
  final GDriveRestoreCandidateRs inner;

  const GDriveRestoreCandidate({required this.inner});

  RootSeed tryDecrypt({required String password}) =>
      AppRs.instance.api.crateFfiGdriveGDriveRestoreCandidateTryDecrypt(
        that: this,
        password: password,
      );

  String userPk() =>
      AppRs.instance.api.crateFfiGdriveGDriveRestoreCandidateUserPk(that: this);

  @override
  int get hashCode => inner.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is GDriveRestoreCandidate &&
          runtimeType == other.runtimeType &&
          inner == other.inner;
}

/// An authenticated Google Drive client used for restoring from backup.
class GDriveRestoreClient {
  final GDriveRestoreClientRs inner;

  const GDriveRestoreClient({required this.inner});

  Future<List<GDriveRestoreCandidate>> findRestoreCandidates({
    required DeployEnv deployEnv,
    required Network network,
    required bool useSgx,
  }) =>
      AppRs.instance.api.crateFfiGdriveGDriveRestoreClientFindRestoreCandidates(
        that: this,
        deployEnv: deployEnv,
        network: network,
        useSgx: useSgx,
      );

  @override
  int get hashCode => inner.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is GDriveRestoreClient &&
          runtimeType == other.runtimeType &&
          inner == other.inner;
}
