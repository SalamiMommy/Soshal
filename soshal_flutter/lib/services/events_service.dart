// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';

/// Events Service
/// Nearby/user events, create, RSVP and check-in.
class EventsService extends ChangeNotifier {
  List<SoshalEvent> _events = [];
  SoshalEvent? _detail;
  List<String> _attendees = [];
  List<EventReminder> _reminders = [];
  String? _lastError;

  List<SoshalEvent> get events => _events;
  SoshalEvent? get detail => _detail;
  List<String> get attendees => _attendees;
  List<EventReminder> get reminders => _reminders;
  String? get lastError => _lastError;

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
      ),
    );
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
      _lastError = null;
      notifyListeners();
      return _detail!;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
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
      _lastError = null;
      return eventId;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
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
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
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
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  Future<List<String>> getAttendees(String eventId) async {
    try {
      _attendees = RustLib.instance.api.crateFfiEventsEventsGetAttendees(
        eventId: eventId,
      );
      _lastError = null;
      notifyListeners();
      return _attendees;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  Future<List<EventReminder>> fetchReminders() async {
    try {
      final json = RustLib.instance.api.crateFfiEventsEventsRemindersList();
      _reminders = (jsonDecode(json) as List<dynamic>)
          .map((e) => EventReminder.fromJson(e as Map<String, dynamic>))
          .toList();
      _lastError = null;
      notifyListeners();
      return _reminders;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
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
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
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
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  Future<List<SoshalEvent>> _decode(String Function() call) async {
    try {
      final json = call();
      final decoded = jsonDecode(json);
      final parsed = (decoded as List<dynamic>)
          .map((e) => SoshalEvent.fromJson(e as Map<String, dynamic>))
          .toList();
      _events = parsed.length > 100 ? parsed.sublist(0, 100) : parsed;
      _lastError = null;
      notifyListeners();
      return _events;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
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
      id: json['id'] as String? ?? '',
      eventId: json['event_id'] as String? ?? '',
      title: json['title'] as String? ?? '',
      startTime: (json['start_time'] as num?)?.toInt() ?? 0,
      minutesBefore: (json['minutes_before'] as num?)?.toInt() ?? 0,
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
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
      id: json['id'] as String? ?? '',
      creatorPubkey: json['creator_pubkey'] as String? ?? '',
      title: json['title'] as String? ?? '',
      description: json['description'] as String? ?? '',
      location: json['location'] as String? ?? '',
      latitude: (json['latitude'] as num?)?.toDouble() ?? 0,
      longitude: (json['longitude'] as num?)?.toDouble() ?? 0,
      startTime: (json['start_time'] as num?)?.toInt() ?? 0,
      endTime: (json['end_time'] as num?)?.toInt() ?? 0,
      image: json['image'] as String? ?? '',
      attendees: (json['attendees'] as num?)?.toInt() ?? 0,
      rsvpStatus: json['rsvp_status'] as String? ?? '',
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
    );
  }
}
