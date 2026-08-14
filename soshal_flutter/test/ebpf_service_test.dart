// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/ebpf_service.dart';

import './helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-ebpf');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('EbpfService', () {
    test('defaults: enabled, SocketFilterBpf, zero counters', () {
      final ebpf = EbpfService();
      expect(ebpf.enabled, isTrue);
      expect(ebpf.mode, 'SocketFilterBpf');
      expect(ebpf.droppedPackets, 0);
      expect(ebpf.passedPackets, 0);
      expect(ebpf.nanosSaved, 0);
      expect(ebpf.blockedPeersCount, 0);
      expect(api.calls, isEmpty);
    });

    test('blockIp returns FFI result and refreshes stats', () async {
      final ebpf = EbpfService();
      api.stubBool('crateFfiEbpfEbpfBlockIp', true);
      api.stubString(
          'crateFfiEbpfEbpfGetStats',
          '{"mode":"SocketFilterBpf","dropped_packets":12,'
          '"passed_packets":200,"nanos_saved":3000,"blocked_peers_count":1}');

      expect(await ebpf.blockIp('10.0.0.5'), isTrue);

      expect(ebpf.lastError, isNull);
      expect(ebpf.droppedPackets, 12);
      expect(ebpf.passedPackets, 200);
      expect(ebpf.nanosSaved, 3000);
      expect(ebpf.blockedPeersCount, 1);
      final inv = api.callsOf('crateFfiEbpfEbpfBlockIp').single;
      expect(api.namedArg(inv, 'ip'), '10.0.0.5');
      expect(api.callCount('crateFfiEbpfEbpfGetStats'), 1);
    });

    test('blockIp FFI throw returns false and sets lastError', () async {
      final ebpf = EbpfService();
      api.stub('crateFfiEbpfEbpfBlockIp', (_) => throw Exception('no perms'));

      expect(await ebpf.blockIp('10.0.0.6'), isFalse);
      expect(ebpf.lastError, contains('no perms'));
    });

    test('unblockIp forwards ip and updates stats', () async {
      final ebpf = EbpfService();
      api.stubBool('crateFfiEbpfEbpfUnblockIp', true);
      api.stubString('crateFfiEbpfEbpfGetStats',
          '{"mode":"SocketFilterBpf","blocked_peers_count":0}');

      expect(await ebpf.unblockIp('10.0.0.5'), isTrue);
      expect(ebpf.blockedPeersCount, 0);
      final inv = api.callsOf('crateFfiEbpfEbpfUnblockIp').single;
      expect(api.namedArg(inv, 'ip'), '10.0.0.5');
    });

    test('refreshStats parses full stats and clears error', () async {
      final ebpf = EbpfService();
      api.stubString(
          'crateFfiEbpfEbpfGetStats',
          '{"mode":"rate-limit","dropped_packets":5,"passed_packets":10,'
          '"nanos_saved":777,"blocked_peers_count":2}');

      await ebpf.refreshStats();

      expect(ebpf.mode, 'rate-limit');
      expect(ebpf.droppedPackets, 5);
      expect(ebpf.passedPackets, 10);
      expect(ebpf.nanosSaved, 777);
      expect(ebpf.blockedPeersCount, 2);
      expect(ebpf.lastError, isNull);
    });

    test('refreshStats with empty map keeps defaults', () async {
      final ebpf = EbpfService();
      api.stubString('crateFfiEbpfEbpfGetStats', '{}');

      await ebpf.refreshStats();

      expect(ebpf.mode, 'SocketFilterBpf');
      expect(ebpf.droppedPackets, 0);
      expect(ebpf.nanosSaved, 0);
      expect(ebpf.lastError, isNull);
    });

    test('refreshStats with malformed json sets lastError without throwing',
        () async {
      final ebpf = EbpfService();
      api.stubString('crateFfiEbpfEbpfGetStats', 'not-json');

      await ebpf.refreshStats();

      expect(ebpf.lastError, isNotNull);
      expect(ebpf.mode, 'SocketFilterBpf');
    });
  });
}