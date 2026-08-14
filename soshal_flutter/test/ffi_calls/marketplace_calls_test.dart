// Generated callable ffi tests for marketplace
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/marketplace.dart';
import 'package:soshal_flutter/frb_generated.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-calls-marketplace');
  final api = env.$1;

  test('marketplaceGetTrending calls String marketplaceGetTrending({required int limit}) => RustLib.instance.api', () {
    api.stubString('String marketplaceGetTrending({required int limit}) => RustLib.instance.api', 'stub');
    final res = marketplaceGetTrending(limit}: 1);
    expect(res, 'stub');
    expect(api.callCount('String marketplaceGetTrending({required int limit}) => RustLib.instance.api'), 1);
  });

  test('marketplaceGetOrder calls String marketplaceGetOrder({required String orderId}) => RustLib.instance.api', () {
    api.stubString('String marketplaceGetOrder({required String orderId}) => RustLib.instance.api', 'stub');
    final res = marketplaceGetOrder(orderId}: "x");
    expect(res, 'stub');
    expect(api.callCount('String marketplaceGetOrder({required String orderId}) => RustLib.instance.api'), 1);
  });

  test('marketplaceGetEscrow calls String marketplaceGetEscrow({required String escrowId}) => RustLib.instance.api', () {
    api.stubString('String marketplaceGetEscrow({required String escrowId}) => RustLib.instance.api', 'stub');
    final res = marketplaceGetEscrow(escrowId}: "x");
    expect(res, 'stub');
    expect(api.callCount('String marketplaceGetEscrow({required String escrowId}) => RustLib.instance.api'), 1);
  });

}
