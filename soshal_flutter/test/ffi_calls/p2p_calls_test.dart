// Generated callable ffi tests for p2p
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/p2p.dart';
import 'package:soshal_flutter/frb_generated.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-calls-p2p');
  final api = env.$1;

  test('p2PMdnsBrowseStop calls crateFfiP2PP2PMdnsBrowseStop', () {
    api.stubBool('crateFfiP2PP2PMdnsBrowseStop', true);
    final res = p2PMdnsBrowseStop();
    expect(res, true);
    expect(api.callCount('crateFfiP2PP2PMdnsBrowseStop'), 1);
  });

  test('p2PLanServerPort calls crateFfiP2PP2PLanServerPort', () {
    api.stubInt('crateFfiP2PP2PLanServerPort', 7);
    final res = p2PLanServerPort();
    expect(res, 7);
    expect(api.callCount('crateFfiP2PP2PLanServerPort'), 1);
  });

  test('p2PLanServerStop calls crateFfiP2PP2PLanServerStop', () {
    api.stubBool('crateFfiP2PP2PLanServerStop', true);
    final res = p2PLanServerStop();
    expect(res, true);
    expect(api.callCount('crateFfiP2PP2PLanServerStop'), 1);
  });

  test('p2PQuicServerPort calls crateFfiP2PP2PQuicServerPort', () {
    api.stubInt('crateFfiP2PP2PQuicServerPort', 7);
    final res = p2PQuicServerPort();
    expect(res, 7);
    expect(api.callCount('crateFfiP2PP2PQuicServerPort'), 1);
  });

  test('p2PQuicServerStop calls crateFfiP2PP2PQuicServerStop', () {
    api.stubBool('crateFfiP2PP2PQuicServerStop', true);
    final res = p2PQuicServerStop();
    expect(res, true);
    expect(api.callCount('crateFfiP2PP2PQuicServerStop'), 1);
  });

  test('p2PPowerMode calls crateFfiP2PP2PPowerMode', () {
    api.stubString('crateFfiP2PP2PPowerMode', 'stub');
    final res = p2PPowerMode();
    expect(res, 'stub');
    expect(api.callCount('crateFfiP2PP2PPowerMode'), 1);
  });

  test('p2PStopAll calls crateFfiP2PP2PStopAll', () {
    api.stubBool('crateFfiP2PP2PStopAll', true);
    final res = p2PStopAll();
    expect(res, true);
    expect(api.callCount('crateFfiP2PP2PStopAll'), 1);
  });

}
