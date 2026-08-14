// Manual ffi tests for events
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/events.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-events-manual');
  final api = env.$1;

  test('fetchNearby/create/rsvp/checkin', () {
    api.stubString('crateFfiEventsEventsFetchNearby', '[]');
    api.stubString('crateFfiEventsEventsCreate', '{}');
    api.stubBool('crateFfiEventsEventsRsvp', true);
    api.stubBool('crateFfiEventsEventsCheckIn', true);

    final nearby = eventsFetchNearby(latitude: 0.0, longitude: 0.0, radiusKm: 1.0, limit: 10);
    final evt = eventsCreate(creatorPubkey: 'u', title: 't', description: '', location: '', latitude: 0.0, longitude: 0.0, startTime: BigInt.from(0), endTime: BigInt.from(1), imageUrl: '');
    final r = eventsRsvp(eventId: 'e', userPubkey: 'u', rsvpStatus: 'accepted');
    final c = eventsCheckIn(eventId: 'e', userPubkey: 'u', latitude: 0.0, longitude: 0.0);

    expect(nearby, '[]');
    expect(evt, '{}');
    expect(r, true);
    expect(c, true);
    expect(api.callCount('crateFfiEventsEventsFetchNearby'), 1);
  });
}