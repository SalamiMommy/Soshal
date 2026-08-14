// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/network.dart';
import 'package:soshal_flutter/services/network_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void expectLastError(NetworkService svc, String fragment) {
  expect(svc.lastError, contains(fragment));
}

void main() {
  final env = bootstrapTestEnv('test-network');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  Future<T> fv<T>(T value) => Future.value(value);

  group('NetworkService transport mode', () {
    test('loadTransportMode restores persisted mode and persists it', () async {
      final svc = NetworkService();
      addTearDown(svc.dispose);
      api.stubString('crateFfiDbDbGetSetting', 'i2p');
      api.stubBool('crateFfiNetworkNetworkSetTransportMode', true);
      api.stubBool('crateFfiDbDbSetSetting', true);

      await svc.loadTransportMode();

      expect(svc.transportMode, TransportMode.i2p);
      expect(svc.i2pForced, isTrue);
      var inv = api.callsOf('crateFfiNetworkNetworkSetTransportMode').single;
      expect(api.namedArg(inv, 'mode'), 'i2p');
      inv = api.callsOf('crateFfiDbDbSetSetting').single;
      expect(api.namedArg(inv, 'key'), 'transport_mode');
      expect(api.namedArg(inv, 'value'), 'i2p');
    });

    test('loadTransportMode falls back to Rust-side mode on unknown setting',
        () async {
      final svc = NetworkService();
      addTearDown(svc.dispose);
      api.stubString('crateFfiDbDbGetSetting', 'bogus');
      api.stubString('crateFfiNetworkNetworkGetTransportMode', 'auto');

      await svc.loadTransportMode();

      expect(svc.transportMode, TransportMode.auto);
      expect(svc.i2pForced, isFalse);
      expect(api.callCount('crateFfiNetworkNetworkSetTransportMode'), 0);
    });

    test('loadTransportMode error sets lastError without rethrow', () async {
      final svc = NetworkService();
      addTearDown(svc.dispose);
      api.stub('crateFfiDbDbGetSetting', (_) => throw Exception('db down'));

      await svc.loadTransportMode();

      expectLastError(svc, 'db down');
      expect(svc.transportMode, TransportMode.clearnet);
    });

    test('setTransportMode applies and persists on success', () async {
      final svc = NetworkService();
      addTearDown(svc.dispose);
      api.stubBool('crateFfiNetworkNetworkSetTransportMode', true);
      api.stubBool('crateFfiDbDbSetSetting', true);

      final ok = await svc.setTransportMode(TransportMode.i2p);

      expect(ok, isTrue);
      expect(svc.transportMode, TransportMode.i2p);
      expect(svc.i2pForced, isTrue);
      expect(svc.lastError, isNull);
    });

    test('setTransportMode keeps current mode when bridge rejects', () async {
      final svc = NetworkService();
      addTearDown(svc.dispose);
      api.stubBool('crateFfiNetworkNetworkSetTransportMode', false);
      api.stubBool('crateFfiDbDbSetSetting', true);

      final ok = await svc.setTransportMode(TransportMode.i2p);

      expect(ok, isFalse);
      expect(svc.transportMode, TransportMode.clearnet);
      expect(api.callCount('crateFfiDbDbSetSetting'), 0);
    });

    test('setTransportMode error rethrows and sets lastError', () async {
      final svc = NetworkService();
      addTearDown(svc.dispose);
      api.stub('crateFfiNetworkNetworkSetTransportMode',
          (_) => throw Exception('mode boom'));

      await expectLater(
        svc.setTransportMode(TransportMode.auto),
        throwsException,
      );
      expectLastError(svc, 'mode boom');
    });
  });

  group('NetworkService i2p', () {
    test('startI2pSession and stopI2pSession forward args', () async {
      final svc = NetworkService();
      addTearDown(svc.dispose);
      api.stubString('crateFfiNetworkI2PStartSession', 'i2p-addr');

      final dest = await svc.startI2pSession(destination: 'd1');

      expect(dest, 'i2p-addr');
      var inv = api.callsOf('crateFfiNetworkI2PStartSession').single;
      expect(api.namedArg(inv, 'destination'), 'd1');

      api.stubBool('crateFfiNetworkI2PStopSession', true);
      final ok = await svc.stopI2pSession();
      expect(ok, isTrue);
    });

    test('startI2pSession error rethrows and sets lastError', () async {
      final svc = NetworkService();
      addTearDown(svc.dispose);
      api.stub('crateFfiNetworkI2PStartSession',
          (_) => throw Exception('sam down'));

      await expectLater(svc.startI2pSession(), throwsException);
      expectLastError(svc, 'sam down');
    });

    test('i2pSessionStatus decodes JSON map', () async {
      final svc = NetworkService();
      addTearDown(svc.dispose);
      api.stubString(
        'crateFfiNetworkI2PSessionStatus',
        '{"running":true,"destination":"abc"}',
      );

      final map = await svc.i2pSessionStatus();

      expect(map['running'], isTrue);
      expect(map['destination'], 'abc');
      expect(svc.lastError, isNull);
    });
  });

  group('NetworkService status', () {
    test('refresh reads i2p and freenet presence', () async {
      final svc = NetworkService();
      addTearDown(svc.dispose);
      api.stub('crateFfiNetworkNetworkI2PStatus', (_) => fv(true));
      api.stub('crateFfiNetworkNetworkFreenetStatus', (_) => fv(false));

      await svc.refresh();

      expect(svc.i2p, isTrue);
      expect(svc.freenet, isFalse);
      expect(svc.lastError, isNull);
    });

    test('refresh error sets lastError without rethrow', () async {
      final svc = NetworkService();
      addTearDown(svc.dispose);
      api.stub('crateFfiNetworkNetworkI2PStatus',
          (_) => throw Exception('status boom'));

      await svc.refresh();

      expectLastError(svc, 'status boom');
      expect(svc.i2p, isNull);
    });
  });

  group('NetworkService relays', () {
    test('fetchRelayStatus parses relay list; reinitRelays re-uses urls',
        () async {
      final svc = NetworkService();
      addTearDown(svc.dispose);
      api.stub(
        'crateFfiNetworkNetworkGetRelayStatus',
        (_) => fv('[{"url":"wss://r1","connected":true,'
            '"latency_ms":12,"last_event_at":99}]'),
      );

      final relays = await svc.fetchRelayStatus();

      expect(relays.length, 1);
      expect(svc.relays.first.url, 'wss://r1');
      expect(svc.relays.first.connected, isTrue);
      expect(svc.relays.first.latencyMs, 12);
      expect(svc.relays.first.lastEventAt, 99);

      api.stub('crateFfiNetworkNetworkInitRelays', (_) => fv('ok'));
      await svc.reinitRelays();
      var inv = api.callsOf('crateFfiNetworkNetworkInitRelays').single;
      expect(api.namedArg(inv, 'relayUrls'), ['wss://r1']);
    });

    test('reinitRelays no-ops when no relays loaded', () async {
      final svc = NetworkService();
      addTearDown(svc.dispose);

      await svc.reinitRelays();

      expect(api.callCount('crateFfiNetworkNetworkInitRelays'), 0);
    });

    test('addRelay and removeRelay forward url', () async {
      final svc = NetworkService();
      addTearDown(svc.dispose);
      api.stub('crateFfiNetworkNetworkAddRelay', (_) => fv(true));

      final ok = await svc.addRelay('wss://r2');

      expect(ok, isTrue);
      var inv = api.callsOf('crateFfiNetworkNetworkAddRelay').single;
      expect(api.namedArg(inv, 'url'), 'wss://r2');

      api.stub('crateFfiNetworkNetworkRemoveRelay', (_) => fv(false));
      final removed = await svc.removeRelay('wss://r2');
      expect(removed, isFalse);
      inv = api.callsOf('crateFfiNetworkNetworkRemoveRelay').single;
      expect(api.namedArg(inv, 'url'), 'wss://r2');
    });

    test('initRelays returns bridge result', () async {
      final svc = NetworkService();
      addTearDown(svc.dispose);
      api.stub('crateFfiNetworkNetworkInitRelays', (_) => fv('connected'));

      final result = await svc.initRelays(['wss://a', 'wss://b']);

      expect(result, 'connected');
      final inv = api.callsOf('crateFfiNetworkNetworkInitRelays').single;
      expect(api.namedArg(inv, 'relayUrls'), ['wss://a', 'wss://b']);
    });

    test('fetchRelayStatus error rethrows and sets lastError', () async {
      final svc = NetworkService();
      addTearDown(svc.dispose);
      api.stub('crateFfiNetworkNetworkGetRelayStatus',
          (_) => throw Exception('relay boom'));

      await expectLater(svc.fetchRelayStatus(), throwsException);
      expectLastError(svc, 'relay boom');
    });
  });

  group('NetworkService diagnostics', () {
    test('fetchHttp3 passes url/method/headers/body and returns dto', () async {
      final svc = NetworkService();
      addTearDown(svc.dispose);
      api.stub(
        'crateFfiNetworkNetworkFetchHttp3',
        (_) => fv(HttpResponseDto(
          status: 200,
          body: Uint8List.fromList([1, 2, 3]),
        )),
      );

      final resp = await svc.fetchHttp3(
        'https://x.example/p',
        method: 'POST',
        headers: {'a': 'b'},
        body: Uint8List.fromList([9]),
      );

      expect(resp.status, 200);
      expect(resp.body, [1, 2, 3]);
      final inv = api.callsOf('crateFfiNetworkNetworkFetchHttp3').single;
      expect(api.namedArg(inv, 'url'), 'https://x.example/p');
      expect(api.namedArg(inv, 'method'), 'POST');
      expect(api.namedArg(inv, 'headersJson'), jsonEncode({'a': 'b'}));
      expect(api.namedArg(inv, 'body'), [9]);
      expect(svc.lastError, isNull);
    });

    test('fetchHttp3 error rethrows and sets lastError', () async {
      final svc = NetworkService();
      addTearDown(svc.dispose);
      api.stub('crateFfiNetworkNetworkFetchHttp3',
          (_) => throw Exception('http3 down'));

      await expectLater(svc.fetchHttp3('https://x'), throwsException);
      expectLastError(svc, 'http3 down');
    });

    test('fetchMultiBearerStatus decodes JSON map', () async {
      final svc = NetworkService();
      addTearDown(svc.dispose);
      api.stub(
        'crateFfiNetworkNetworkGetMultiBearerStatus',
        (_) => fv('{"ble":true,"wifi_direct":false}'),
      );

      final map = await svc.fetchMultiBearerStatus('pk-me');

      expect(map['ble'], isTrue);
      expect(map['wifi_direct'], isFalse);
      final inv =
          api.callsOf('crateFfiNetworkNetworkGetMultiBearerStatus').single;
      expect(api.namedArg(inv, 'ownPubkey'), 'pk-me');
    });

    test('fetchSysDiagnostics returns raw JSON synchronously', () {
      final svc = NetworkService();
      addTearDown(svc.dispose);
      api.stubString(
        'crateFfiNetworkNetworkGetSysDiagnostics',
        '{"engine":"sqlite"}',
      );

      final json = svc.fetchSysDiagnostics();

      expect(json, '{"engine":"sqlite"}');
      expect(api.callCount('crateFfiNetworkNetworkGetSysDiagnostics'), 1);
    });

    test('reconcileProllyTree decodes success map and passes args', () async {
      final svc = NetworkService();
      addTearDown(svc.dispose);
      api.stub(
        'crateFfiNetworkNetworkReconcileProllyTree',
        (_) => fv('{"success":true,"prolly_root":"r9"}'),
      );

      final res = await svc.reconcileProllyTree(
        localKv: {'a': 'b'},
        remoteRootHash: 'rh',
      );

      expect(res['success'], isTrue);
      expect(res['prolly_root'], 'r9');
      final inv =
          api.callsOf('crateFfiNetworkNetworkReconcileProllyTree').single;
      expect(api.namedArg(inv, 'localKvJson'), jsonEncode({'a': 'b'}));
      expect(api.namedArg(inv, 'remoteRootHash'), 'rh');
      expect(svc.lastError, isNull);
    });

    test('reconcileProllyTree failure returns fallback map', () async {
      final svc = NetworkService();
      addTearDown(svc.dispose);
      api.stub('crateFfiNetworkNetworkReconcileProllyTree',
          (_) => throw Exception('sync down'));

      final res = await svc.reconcileProllyTree(
        localKv: const {},
        remoteRootHash: 'rh',
      );

      expect(res['success'], isFalse);
      expect(res['error_msg'], contains('sync down'));
    });
  });
}