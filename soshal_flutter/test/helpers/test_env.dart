import 'package:flutter_test/flutter_test.dart';
import 'package:path_provider_platform_interface/path_provider_platform_interface.dart';
import 'package:plugin_platform_interface/plugin_platform_interface.dart';

import 'fake_api.dart';

export 'fake_api.dart';

class FakePathProvider extends PathProviderPlatform
    with MockPlatformInterfaceMixin {
  final String root;

  FakePathProvider(this.root);

  @override
  Future<String?> getApplicationDocumentsPath() async => root;

  @override
  Future<String?> getApplicationSupportPath() async =>
      '$root/Library/Application Support';

  @override
  Future<String?> getTemporaryPath() async => '$root/tmp';
}

/// Bootstrap a test file: flutter binding, fake FFI api and a fake
/// documents directory under [tmpRoot].
///
/// Returns the FakeApi INSTALLED (and exported for stubbing), plus the
/// documents directory path used for path_provider calls.
(FakeApi, String) bootstrapTestEnv(String tmpRoot) {
  TestWidgetsFlutterBinding.ensureInitialized();
  final api = FakeApi();
  installFakeApi(api);
  PathProviderPlatform.instance = FakePathProvider(tmpRoot);
  return (api, tmpRoot);
}