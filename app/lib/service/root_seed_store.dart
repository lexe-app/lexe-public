library;

import 'package:app_rs_dart/ffi/secret_store.dart' show SecretStore;
import 'package:app_rs_dart/ffi/types.dart' show Config, RootSeed;

abstract interface class RootSeedStore {
  RootSeed? readRootSeed();
}

final class SecretStoreRootSeedStore implements RootSeedStore {
  SecretStoreRootSeedStore({required Config config})
    : _secretStore = SecretStore(config: config);

  final SecretStore _secretStore;

  @override
  RootSeed? readRootSeed() => this._secretStore.readRootSeed();
}
