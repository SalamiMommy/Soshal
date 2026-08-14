// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/mesh_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-mesh');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  MeshService service({String? pubkey = 'pk-1'}) =>
      MeshService(pubkey: () => pubkey);

  const statusJson =
      '{"running":true,"destination_hash":"d-hash-1","active_routes":3,'
      '"rx_packets":10,"tx_packets":4}';

  group('MeshService', () {
    test('startTransport parses status and passes pubkey/bindAddr', () async {
      final mesh = service();
      api.stubString('crateFfiNetworkReticulumStartTransport', statusJson);

      await mesh.startTransport(bindAddr: '0.0.0.0:9999');

      expect(mesh.running, isTrue);
      expect(mesh.destinationHash, 'd-hash-1');
      expect(mesh.activeRoutes, 3);
      expect(mesh.rxPackets, 10);
      expect(mesh.txPackets, 4);
      expect(mesh.lastError, isNull);
      final inv =
          api.callsOf('crateFfiNetworkReticulumStartTransport').single;
      expect(api.namedArg(inv, 'pubkey'), 'pk-1');
      expect(api.namedArg(inv, 'bindAddr'), '0.0.0.0:9999');
    });

    test('no active pubkey throws before any FFI call', () async {
      final mesh = service(pubkey: null);

      await expectLater(mesh.startTransport(), throwsException);
      expect(api.calls, isEmpty);
    });

    test('startAutoInterface forwards enabled/port/intervalMs', () async {
      final mesh = service();
      api.stubString('crateFfiNetworkReticulumStartAutoInterface', statusJson);

      await mesh.startAutoInterface(enabled: true, port: 1234, intervalMs: 500);

      final inv =
          api.callsOf('crateFfiNetworkReticulumStartAutoInterface').single;
      expect(api.namedArg(inv, 'enabled'), true);
      expect(api.namedArg(inv, 'port'), 1234);
      expect(api.namedArg(inv, 'intervalMs'), 500);
      expect(mesh.running, isTrue);
    });

    test('startTcpServer forwards port/maxConnections', () async {
      final mesh = service();
      api.stubString('crateFfiNetworkReticulumStartTcpServer', statusJson);

      await mesh.startTcpServer(port: 4321, maxConnections: 8);

      final inv =
          api.callsOf('crateFfiNetworkReticulumStartTcpServer').single;
      expect(api.namedArg(inv, 'port'), 4321);
      expect(api.namedArg(inv, 'maxConnections'), 8);
    });

    test('sendPacket returns FFI bool and passes args', () async {
      final mesh = service();
      api.stubBool('crateFfiNetworkReticulumSendPacket', true);

      expect(
        await mesh.sendPacket(destAddr: 'dest-1', packetJson: '{}'),
        isTrue,
      );
      expect(mesh.lastError, isNull);
      final inv = api.callsOf('crateFfiNetworkReticulumSendPacket').single;
      expect(api.namedArg(inv, 'destAddr'), 'dest-1');
      expect(api.namedArg(inv, 'packetJson'), '{}');
    });

    test('requestLink and addressFromPubkey return raw json', () async {
      final mesh = service();
      api.stubString('crateFfiNetworkReticulumRequestLink', 'link-ok');
      api.stubString(
          'crateFfiNetworkReticulumAddressFromPubkey', 'addr-hex-1');

      expect(await mesh.requestLink('abcd'), 'link-ok');
      expect(await mesh.addressFromPubkey(), 'addr-hex-1');
      expect(mesh.lastError, isNull);
    });

    test('addressFromAspect forwards appName/aspect', () async {
      final mesh = service();
      api.stubString(
          'crateFfiNetworkReticulumAddressFromAspect', 'aspect-addr');

      expect(await mesh.addressFromAspect('soshal', 'chat'), 'aspect-addr');
      final inv =
          api.callsOf('crateFfiNetworkReticulumAddressFromAspect').single;
      expect(api.namedArg(inv, 'appName'), 'soshal');
      expect(api.namedArg(inv, 'aspect'), 'chat');
    });

    test('announce parses status and passes optional aspect', () async {
      final mesh = service();
      api.stubString('crateFfiNetworkReticulumCreateAnnounce', statusJson);

      await mesh.announce('chat');

      expect(mesh.running, isTrue);
      final inv =
          api.callsOf('crateFfiNetworkReticulumCreateAnnounce').single;
      expect(api.namedArg(inv, 'aspect'), 'chat');
    });

    test('refreshStatus updates all counters', () async {
      final mesh = service();
      api.stubString('crateFfiNetworkReticulumGetStatus',
          '{"running":false,"active_routes":7,"rx_packets":99}');

      await mesh.refreshStatus();

      expect(mesh.running, isFalse);
      expect(mesh.activeRoutes, 7);
      expect(mesh.rxPackets, 99);
      expect(mesh.lastError, isNull);
    });

    test('partial status payload keeps prior fields', () async {
      final mesh = service();
      api.stubString('crateFfiNetworkReticulumGetStatus', statusJson);
      await mesh.refreshStatus();
      expect(mesh.txPackets, 4);

      api.stubString(
          'crateFfiNetworkReticulumGetStatus', '{"running":false}');
      await mesh.refreshStatus();

      expect(mesh.running, isFalse);
      expect(mesh.destinationHash, 'd-hash-1', reason: 'field absent keeps old');
      expect(mesh.txPackets, 4);
    });

    test('FFI throw sets lastError and rethrows', () async {
      final mesh = service();
      api.stub('crateFfiNetworkReticulumGetStatus',
          (_) => throw Exception('mesh down'));

      await expectLater(mesh.refreshStatus(), throwsException);
      expect(mesh.lastError, contains('mesh down'));
    });

    test('malformed status json lands in lastError without throwing',
        () async {
      final mesh = service();
      api.stubString('crateFfiNetworkReticulumGetStatus', 'not-json');

      await mesh.refreshStatus();

      expect(mesh.lastError, isNotNull);
      expect(mesh.running, isFalse);
    });
  });
}