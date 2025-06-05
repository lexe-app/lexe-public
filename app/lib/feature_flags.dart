/// Enable certain app features based on the user's `userPk`.
library;

import 'package:app_rs_dart/ffi/types.dart' show DeployEnv;
import 'package:flutter/foundation.dart' show immutable;

/// Enable all experimental features for Lexe devs : )
const Set<String> prodLexeDevUserPks = {
  // @phlip9
  "8cb9479ef8aadec450b64e14daeafe811af881bf6e261229b0658777bfafc686",
  // @maxfangx
  "b484a4890b47358ee68684bcd502d2eefa1bc66cc0f8ac2e5f06384676be74eb",
  // Lexe iOS dev device
  "7b99af3b38ccec46bd691d1f091053868dc70e5a3945908825beb0271cfe01c9",
};

@immutable
final class FeatureFlags {
  /// Determine the enabled feature flags based on the deployment environment
  /// and `userPk`.
  factory FeatureFlags({
    required DeployEnv deployEnv,
    required String userPk,
  }) {
    switch (deployEnv) {
      case DeployEnv.dev:
        return const FeatureFlags.all();
      case DeployEnv.staging:
        return const FeatureFlags.all();
      case DeployEnv.prod:
        if (prodLexeDevUserPks.contains(userPk)) {
          return const FeatureFlags.all();
        } else {
          return const FeatureFlags.none();
        }
    }
  }

  /// Disable all features.
  const FeatureFlags.none({
    this.showBolt12OffersRecvPage = false,
  });

  /// Enable all features.
  const FeatureFlags.all({
    this.showBolt12OffersRecvPage = true,
  });

  /// On the Wallet > Receive page, show the experimental BOLT12 offer receive
  /// QR code.
  final bool showBolt12OffersRecvPage;
}
