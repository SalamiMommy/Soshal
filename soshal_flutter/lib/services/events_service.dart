// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import '../utils/json_ext.dart';
import '../utils/offthread.dart';
import 'error_log.dart';

/// Events Service
/// Nearby/user events, create, RSVP and check-in.
class EventsService extends ChangeNotifier with LastErrorMixin, DeferredNotify {
  List<SoshalEvent> _events = [];
  SoshalEvent? _detail;
  List<String> _attendees = [];
  List<EventReminder> _reminders = [];
  Map<String, double> _scores = {};

  List<SoshalEvent> get events => _events;
  SoshalEvent? get detail => _detail;
  List<String> get attendees => _attendees;
  List<EventReminder> get reminders => _reminders;
  Map<String, double> get scores => _scores;

  /// Clear all account-scoped state on account switch so Account B never
  /// sees Account A's cached events, detail, attendees, reminders, or scores.
  void resetForAccountSwitch() {
    _events = [];
    _detail = null;
    _attendees = [];
    _reminders = [];
    _scores = {};
    clearLastError();
    notifyListeners();
  }

  List<EventReminder> remindersForEvent(String eventId) =>
      _reminders.where((r) => r.eventId == eventId).toList();

  Future<List<SoshalEvent>> fetchNearby(
      {double latitude = 0,
      double longitude = 0,
      double radiusKm = 0,
      int limit = 50}) async {
    return _decode(
      () => RustLib.instance.api.crateFfiEventsEventsFetchNearby(
        latitude: latitude,
        longitude: longitude,
        radiusKm: radiusKm,
        limit: limit,
        audience: 'public',
      ),
    );
  }

  /// Encodes lat/lng into a geohash string (sync FFI; spatial-core).
  String encodeGeohash({required double lat, required double lon}) {
    try {
      final geohash = RustLib.instance.api
          .crateFfiSpatialSpatialEncodeGeohash(lat: lat, lon: lon);
      clearLastError();
      notifyDeferred();
      return geohash;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<List<SoshalEvent>> fetchUserEvents(String userPubkey,
      {int limit = 50}) async {
    return _decode(
      () => RustLib.instance.api.crateFfiEventsEventsFetchUserEvents(
        userPubkey: userPubkey,
        limit: limit,
      ),
    );
  }

  Future<SoshalEvent> getEvent(String eventId) async {
    try {
      final json = RustLib.instance.api.crateFfiEventsEventsGetEvent(
        eventId: eventId,
      );
      _detail = SoshalEvent.fromJson(jsonDecode(json));
      clearLastError();
      notifyDeferred();
      return _detail!;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<String> create(
    String creatorPubkey,
    String title,
    String description,
    String location,
    double latitude,
    double longitude,
    int startTime,
    int endTime,
    String imageUrl,
  ) async {
    try {
      final eventId = RustLib.instance.api.crateFfiEventsEventsCreate(
        creatorPubkey: creatorPubkey,
        title: title,
        description: description,
        location: location,
        latitude: latitude,
        longitude: longitude,
        startTime: BigInt.from(startTime),
        endTime: BigInt.from(endTime),
        imageUrl: imageUrl,
      );
      clearLastError();
      return eventId;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<bool> rsvp(String eventId, String userPubkey, String status) async {
    try {
      final ok = RustLib.instance.api.crateFfiEventsEventsRsvp(
        eventId: eventId,
        userPubkey: userPubkey,
        rsvpStatus: status,
      );
      clearLastError();
      notifyDeferred();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<bool> checkIn(String eventId, String userPubkey, double latitude,
      double longitude) async {
    try {
      final ok = RustLib.instance.api.crateFfiEventsEventsCheckIn(
        eventId: eventId,
        userPubkey: userPubkey,
        latitude: latitude,
        longitude: longitude,
      );
      clearLastError();
      notifyDeferred();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<List<String>> getAttendees(String eventId) async {
    try {
      _attendees = RustLib.instance.api.crateFfiEventsEventsGetAttendees(
        eventId: eventId,
      );
      clearLastError();
      notifyDeferred();
      return _attendees;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<List<EventReminder>> fetchReminders() async {
    try {
      final json = RustLib.instance.api.crateFfiEventsEventsRemindersList();
      _reminders = (jsonDecode(json) as List<dynamic>)
          .map((e) => EventReminder.fromJson(e as Map<String, dynamic>))
          .toList();
      clearLastError();
      notifyDeferred();
      return _reminders;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<String> upsertReminder({
    required String reminderId,
    required String eventId,
    required String title,
    required int startTime,
    required int minutesBefore,
  }) async {
    try {
      final id = RustLib.instance.api.crateFfiEventsEventsReminderUpsert(
        reminderId: reminderId,
        eventId: eventId,
        title: title,
        startTime: startTime,
        minutesBefore: minutesBefore,
      );
      await fetchReminders();
      return id;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<bool> deleteReminder(String reminderId) async {
    try {
      final ok = RustLib.instance.api.crateFfiEventsEventsReminderDelete(
        reminderId: reminderId,
      );
      await fetchReminders();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<List<SoshalEvent>> _decode(String Function() call) async {
    try {
      final json = call();
      final parsed = await runOffThread(() => _parseEvents(json));
      _events = parsed.length > 100 ? parsed.sublist(0, 100) : parsed;
      clearLastError();
      notifyDeferred();
      return _events;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Score loaded events against my interests via their hashtags.
  Future<void> scoreEvents(List<String> myInterests) async {
    final out = <String, double>{};
    final payload = _events
        .map((e) => {
              'id': e.id,
              'title': e.title,
              'description': e.description,
            })
        .toList();
    try {
      final json = RustLib.instance.api.crateFfiEventsEventsScoreEvents(
        eventsJson: jsonEncode(payload),
        myInterestsJson: jsonEncode(myInterests),
      );
      final decoded = jsonDecode(json) as Map<String, dynamic>;
      for (final entry in decoded.entries) {
        out[entry.key] = (entry.value as num?)?.toDouble() ?? 0.0;
      }
    } catch (e, st) {
      setLastError(e, st);
    }
    _scores = out;
    notifyDeferred();
  }
}

/// Locally-saved event reminder (fire time = start_time - minutes_before).
class EventReminder {
  final String id;
  final String eventId;
  final String title;
  final int startTime;
  final int minutesBefore;
  final int createdAt;

  EventReminder({
    required this.id,
    required this.eventId,
    required this.title,
    required this.startTime,
    required this.minutesBefore,
    required this.createdAt,
  });

  factory EventReminder.fromJson(Map<String, dynamic> json) {
    return EventReminder(
      id: json.strOf('id'),
      eventId: json.strOf('event_id'),
      title: json.strOf('title'),
      startTime: json.intOf('start_time'),
      minutesBefore: json.intOf('minutes_before'),
      createdAt: json.intOf('created_at'),
    );
  }

  DateTime get fireAt => DateTime.fromMillisecondsSinceEpoch(
          (startTime - minutesBefore * 60) * 1000)
      .toLocal();
}

/// Locally-stored calendar event.
class SoshalEvent {
  final String id;
  final String creatorPubkey;
  final String title;
  final String description;
  final String location;
  final double latitude;
  final double longitude;
  final int startTime;
  final int endTime;
  final String image;
  final int attendees;
  final String rsvpStatus;
  final int createdAt;

  SoshalEvent({
    required this.id,
    required this.creatorPubkey,
    required this.title,
    required this.description,
    required this.location,
    required this.latitude,
    required this.longitude,
    required this.startTime,
    required this.endTime,
    required this.image,
    required this.attendees,
    required this.rsvpStatus,
    required this.createdAt,
  });

  factory SoshalEvent.fromJson(Map<String, dynamic> json) {
    return SoshalEvent(
      id: json.strOf('id'),
      creatorPubkey: json.strOf('creator_pubkey'),
      title: json.strOf('title'),
      description: json.strOf('description'),
      location: json.strOf('location'),
      latitude: (json['latitude'] as num?)?.toDouble() ?? 0,
      longitude: (json['longitude'] as num?)?.toDouble() ?? 0,
      startTime: json.intOf('start_time'),
      endTime: json.intOf('end_time'),
      image: json.strOf('image'),
      attendees: json.intOf('attendees'),
      rsvpStatus: json.strOf('rsvp_status'),
      createdAt: json.intOf('created_at'),
    );
  }
}

/// JSON → [SoshalEvent] list, top-level so [runOffThread] can decode on a
/// background isolate.
List<SoshalEvent> _parseEvents(String json) {
  final decoded = jsonDecode(json);
  return (decoded as List<dynamic>)
      .map((e) => SoshalEvent.fromJson(e as Map<String, dynamic>))
      .toList();
}
