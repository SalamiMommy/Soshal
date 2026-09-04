// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/events_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('events-svc');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('EventsService', () {
    String eventJson() => jsonEncode({
          'id': 'ev-1',
          'creator_pubkey': 'pk-me',
          'title': 'Sats Meetup',
          'description': 'bitcoin talk',
          'location': 'Berlin',
          'latitude': 52.52,
          'longitude': 13.405,
          'start_time': 1700000000,
          'end_time': 1700003600,
          'image': 'https://x/e.png',
          'attendees': 12,
          'rsvp_status': 'going',
          'created_at': 1699990000,
        });

    test('fetchNearby parses list and passes coords', () async {
      final events = EventsService();
      final evts = List.generate(3, (i) => {
            'id': 'ev-$i',
            'creator_pubkey': 'pk-$i',
            'title': 'event $i',
            'description': '',
            'location': '',
            'latitude': 52.5 + i,
            'longitude': 13.4 + i,
            'start_time': 1700000000 + i,
            'end_time': 1700003600 + i,
            'image': '',
            'attendees': i,
            'rsvp_status': i == 0 ? 'going' : '',
            'created_at': 1699990000,
          });
      api.stubString('crateFfiEventsEventsFetchNearby', jsonEncode(evts));

      final result = await events.fetchNearby(
        latitude: 52.5,
        longitude: 13.4,
        radiusKm: 25,
        limit: 10,
      );
      expect(result.length, 3);
      expect(result.first.id, 'ev-0');
      expect(result.first.title, 'event 0');
      expect(result.first.rsvpStatus, 'going');
      expect(result.first.attendees, 0);
      expect(events.events, result, reason: 'state getter updated');
      expect(events.lastError, isNull);
      final inv = api.callsOf('crateFfiEventsEventsFetchNearby').single;
      expect(api.namedArg(inv, 'latitude'), 52.5);
      expect(api.namedArg(inv, 'longitude'), 13.4);
      expect(api.namedArg(inv, 'radiusKm'), 25);
      expect(api.namedArg(inv, 'limit'), 10);
    });

    test('fetchUserEvents passes userPubkey and limit', () async {
      final events = EventsService();
      api.stubString('crateFfiEventsEventsFetchUserEvents', '[]');

      final result = await events.fetchUserEvents('pk-me', limit: 5);
      expect(result, isEmpty);
      final inv =
          api.callsOf('crateFfiEventsEventsFetchUserEvents').single;
      expect(api.namedArg(inv, 'userPubkey'), 'pk-me');
      expect(api.namedArg(inv, 'limit'), 5);
    });

    test('getEvent sets detail and parses rsvp status', () async {
      final events = EventsService();
      api.stubString('crateFfiEventsEventsGetEvent', eventJson());

      final event = await events.getEvent('ev-1');
      expect(event.id, 'ev-1');
      expect(event.title, 'Sats Meetup');
      expect(event.rsvpStatus, 'going');
      expect(event.latitude, 52.52);
      expect(events.detail?.id, 'ev-1', reason: 'detail getter updated');
      final inv = api.callsOf('crateFfiEventsEventsGetEvent').single;
      expect(api.namedArg(inv, 'eventId'), 'ev-1');
    });

    test('create returns event id and passes all named args', () async {
      final events = EventsService();
      api.stubString('crateFfiEventsEventsCreate', 'ev-9');

      final id = await events.create(
        'pk-me',
        'Meetup',
        'desc',
        'Berlin',
        52.52,
        13.405,
        1700000000,
        1700003600,
        'https://x/e.png',
      );
      expect(id, 'ev-9');
      final inv = api.callsOf('crateFfiEventsEventsCreate').single;
      expect(api.namedArg(inv, 'creatorPubkey'), 'pk-me');
      expect(api.namedArg(inv, 'title'), 'Meetup');
      expect(api.namedArg(inv, 'description'), 'desc');
      expect(api.namedArg(inv, 'location'), 'Berlin');
      expect(api.namedArg(inv, 'latitude'), 52.52);
      expect(api.namedArg(inv, 'longitude'), 13.405);
      expect(api.namedArg(inv, 'startTime'), BigInt.from(1700000000));
      expect(api.namedArg(inv, 'endTime'), BigInt.from(1700003600));
      expect(api.namedArg(inv, 'imageUrl'), 'https://x/e.png');
    });

    test('rsvp and checkIn pass eventId and pubkey', () async {
      final events = EventsService();
      api.stubBool('crateFfiEventsEventsRsvp', true);
      api.stubBool('crateFfiEventsEventsCheckIn', true);

      expect(await events.rsvp('ev-1', 'pk-me', 'going'), isTrue);
      expect(await events.checkIn('ev-1', 'pk-me', 52.52, 13.405), isTrue);
      final rsvpInv = api.callsOf('crateFfiEventsEventsRsvp').single;
      expect(api.namedArg(rsvpInv, 'eventId'), 'ev-1');
      expect(api.namedArg(rsvpInv, 'userPubkey'), 'pk-me');
      expect(api.namedArg(rsvpInv, 'rsvpStatus'), 'going');
      final checkInv = api.callsOf('crateFfiEventsEventsCheckIn').single;
      expect(api.namedArg(checkInv, 'eventId'), 'ev-1');
      expect(api.namedArg(checkInv, 'userPubkey'), 'pk-me');
      expect(api.namedArg(checkInv, 'latitude'), 52.52);
    });

    test('getAttendees populates attendee list state', () async {
      final events = EventsService();
      api.stubListString(
        'crateFfiEventsEventsGetAttendees',
        ['pk-1', 'pk-2'],
      );

      final attendees = await events.getAttendees('ev-1');
      expect(attendees, ['pk-1', 'pk-2']);
      expect(events.attendees, attendees,
          reason: 'attendees getter updated');
      final inv = api.callsOf('crateFfiEventsEventsGetAttendees').single;
      expect(api.namedArg(inv, 'eventId'), 'ev-1');
    });

    test('fetchReminders parses reminders into state', () async {
      final events = EventsService();
      api.stubString(
        'crateFfiEventsEventsRemindersList',
        jsonEncode([
          {
            'id': 'r-1',
            'event_id': 'ev-1',
            'title': 'remind me',
            'start_time': 1700000000,
            'minutes_before': 30,
            'created_at': 1699990000,
          }
        ]),
      );

      final reminders = await events.fetchReminders();
      expect(reminders.single.eventId, 'ev-1');
      expect(reminders.single.title, 'remind me');
      expect(reminders.single.minutesBefore, 30);
      expect(events.reminders, reminders, reason: 'reminders getter updated');
      expect(events.remindersForEvent('ev-1').single.id, 'r-1');
    });

    test('fetchNearby error sets lastError and rethrows', () async {
      final events = EventsService();
      api.stub('crateFfiEventsEventsFetchNearby',
          (_) => throw Exception('events down'));

      await expectLater(events.fetchNearby(), throwsException);
      expect(events.lastError, contains('events down'));
    });

    test('create error sets lastError and rethrows', () async {
      final events = EventsService();
      api.stub('crateFfiEventsEventsCreate',
          (_) => throw Exception('create boom'));

      await expectLater(
        events.create('pk-me', 't', 'd', '', 0, 0, 1, 2, ''),
        throwsException,
      );
      expect(events.lastError, contains('create boom'));
    });

    test('rsvp error sets lastError and rethrows', () async {
      final events = EventsService();
      api.stub('crateFfiEventsEventsRsvp', (_) => throw Exception('rsvp no'));

      await expectLater(events.rsvp('ev-1', 'pk-me', 'going'), throwsException);
      expect(events.lastError, contains('rsvp no'));
    });
  });
}
