// This file is automatically generated, so please do not edit it.
// Generated by `flutter_rust_bridge`@ 2.2.0.

//
// From: `dart_preamble` in `app-rs-codegen/src/lib.rs`
// ignore_for_file: invalid_internal_annotation, always_use_package_imports, directives_ordering, prefer_const_constructors, sort_unnamed_constructors_first
//

// ignore_for_file: invalid_use_of_internal_member, unused_import, unnecessary_import

import '../frb_generated.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';

class GDriveOauth2Flow {
  final String clientId;
  final String codeVerifier;
  final String redirectUri;
  final String redirectUriScheme;
  final String url;

  const GDriveOauth2Flow({
    required this.clientId,
    required this.codeVerifier,
    required this.redirectUri,
    required this.redirectUriScheme,
    required this.url,
  });

  Future<String> exchange({required String resultUri}) => AppRs.instance.api
      .crateFfiGdriveGDriveOauth2FlowExchange(that: this, resultUri: resultUri);

  static GDriveOauth2Flow init(
          {required String clientId, required String serverClientId}) =>
      AppRs.instance.api.crateFfiGdriveGDriveOauth2FlowInit(
          clientId: clientId, serverClientId: serverClientId);

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
      other is GDriveOauth2Flow &&
          runtimeType == other.runtimeType &&
          clientId == other.clientId &&
          codeVerifier == other.codeVerifier &&
          redirectUri == other.redirectUri &&
          redirectUriScheme == other.redirectUriScheme &&
          url == other.url;
}
