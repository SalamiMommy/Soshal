import 'dart:convert';
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/dating_service.dart';
import '../services/events_service.dart';
import '../services/friends_service.dart';
import '../services/media_service.dart';
import '../services/permissions_service.dart';
import '../services/session_service.dart';
import '../utils/format.dart';
import '../utils/safe_url.dart';
import '../widgets/empty_state.dart';

enum _AudienceMode { all, friends, fof, mine }

/// Event photo: blob:// hashes resolve via the chunk store; http(s) load
/// directly. Falls back to an icon when missing or unresolvable.
class _EventImage extends StatelessWidget {
  const _EventImage({required this.url, this.height, this.iconSize = 24});

  final String url;
  final double? height;
  final double iconSize;

  @override
  Widget build(BuildContext context) {
    final trimmed = url.trim();
    if (trimmed.isEmpty) {
      return Icon(Icons.event, size: iconSize);
    }
    if (trimmed.startsWith('blob://')) {
      final hash = trimmed.substring('blob://'.length);
      return FutureBuilder<String>(
        future: context.read<MediaService>().fetchBlob(hash),
        builder: (context, snap) {
          if (snap.hasData && snap.data!.isNotEmpty) {
            return Image.file(
              File(snap.data!),
              fit: BoxFit.cover,
              height: height,
              errorBuilder: (_, __, ___) =>
                  Icon(Icons.event, size: iconSize),
            );
          }
          return Icon(Icons.event, size: iconSize);
        },
      );
    }
    if (!SafeUrl.isSafeMediaUrl(trimmed)) {
      return Icon(Icons.event, size: iconSize);
    }
    final h = height;
    return Image.network(
      trimmed,
      fit: BoxFit.cover,
      height: h,
      cacheWidth: h == null
          ? null
          : (h * MediaQuery.devicePixelRatioOf(context)).round(),
      errorBuilder: (_, __, ___) => Icon(Icons.event, size: iconSize),
    );
  }
}

/// Events: nearby list, create dialog, detail with RSVP + check-in.
class EventsScreen extends StatefulWidget {
  /// Events screen.
  const EventsScreen({super.key});

  @override
  State<EventsScreen> createState() => _EventsScreenState();
}

class _EventsScreenState extends State<EventsScreen> {
  static const _weekdays = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
  static const _weekdaysFull = [
    'Sunday',
    'Monday',
    'Tuesday',
    'Wednesday',
    'Thursday',
    'Friday',
    'Saturday',
  ];
  static const _monthShort = [
    'Jan',
    'Feb',
    'Mar',
    'Apr',
    'May',
    'Jun',
    'Jul',
    'Aug',
    'Sep',
    'Oct',
    'Nov',
    'Dec',
  ];
  static const _monthNames = [
    'January',
    'February',
    'March',
    'April',
    'May',
    'June',
    'July',
    'August',
    'September',
    'October',
    'November',
    'December',
  ];

  bool _loading = true;

  _AudienceMode _audienceMode = _AudienceMode.all;
  List<String> _friendSet = const [];

  List<SoshalEvent>? _sortedCache;
  List<SoshalEvent>? _sortedCacheKey;
  Map<String, double>? _sortedCacheScoresKey;

  /// Events sorted by score desc, memoized on the service list + scores
  /// identity so score/feed notifies don't re-sort every rebuild.
  List<SoshalEvent> _sortedEvents(
      List<SoshalEvent> events, Map<String, double> scores) {
    if (!identical(_sortedCacheKey, events) ||
        !identical(_sortedCacheScoresKey, scores)) {
      _sortedCache = [...events]
        ..sort((a, b) => (scores[b.id] ?? 0).compareTo(scores[a.id] ?? 0));
      _sortedCacheKey = events;
      _sortedCacheScoresKey = scores;
    }
    return _sortedCache!;
  }

  List<SoshalEvent> _visibleEvents(List<SoshalEvent> events) {
    if (_audienceMode != _AudienceMode.friends &&
        _audienceMode != _AudienceMode.fof) {
      return events;
    }
    return events.where((e) => _friendSet.contains(e.creatorPubkey)).toList();
  }

  String _viewMode = 'calendar';
  int _monthOffset = 0;
  DateTime? _selectedDay;

  @override
  void initState() {
    super.initState();
    _load();
  }

  double _radiusKm = 0;

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      final api = context.read<EventsService>();
      final session = context.read<SessionService>();
      final dating = context.read<DatingService>();
      if (_audienceMode == _AudienceMode.mine && session.activePubkey != null) {
        await api.fetchUserEvents(session.activePubkey!);
      } else {
        await api.fetchNearby(
          radiusKm: _radiusKm,
          limit: _audienceMode == _AudienceMode.friends ||
                  _audienceMode == _AudienceMode.fof
              ? 100
              : 50,
        );
      }
      await _loadFriendSet(session);
      var mine = const <String>[];
      final pk = session.activePubkey;
      if (pk != null) {
        try {
          mine = (await dating.getOwnProfile(pk)).interests;
        } catch (_) {
          mine = const [];
        }
      }
      await api.scoreEvents(mine);
    } catch (e) {
      debugPrint('events load: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  Future<void> _loadFriendSet(SessionService session) async {
    try {
      if (_audienceMode != _AudienceMode.friends &&
          _audienceMode != _AudienceMode.fof) {
        _friendSet = const [];
        return;
      }
      final pk = session.activePubkey;
      if (pk == null) {
        _friendSet = const [];
        return;
      }
      final friends = context.read<FriendsService>();
      var mine = <String>[];
      try {
        mine = (jsonDecode(await friends.fetchFollows(pk)) as List<dynamic>)
            .whereType<String>()
            .toList();
      } catch (_) {
        mine = const [];
      }
      if (_audienceMode == _AudienceMode.friends) {
        _friendSet = mine;
        return;
      }
      final union = <String>{};
      for (final f in mine.take(100)) {
        try {
          union.addAll(
              (jsonDecode(await friends.fetchFollows(f)) as List<dynamic>)
                  .whereType<String>());
        } catch (e) {
          debugPrint('fof follows: $e');
        }
      }
      union.remove(pk);
      _friendSet = union.toList();
    } catch (e) {
      debugPrint('friend set: $e');
      _friendSet = const [];
    }
  }

  Future<void> _createDialog() async {
    final title = TextEditingController();
    final desc = TextEditingController();
    final loc = TextEditingController();
    final now = DateTime.now();
    var start = now.add(const Duration(hours: 1));
    var end = start.add(const Duration(hours: 2));
    String imageUrl = '';
    String? pickedPath;

    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, setDialogState) => AlertDialog(
          title: const Text('Create event'),
          content: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                TextField(
                  controller: title,
                  decoration: const InputDecoration(labelText: 'Title *'),
                ),
                TextField(
                  controller: desc,
                  maxLines: 3,
                  decoration: const InputDecoration(labelText: 'Description'),
                ),
                TextField(
                  controller: loc,
                  decoration: const InputDecoration(labelText: 'Location'),
                ),
                ListTile(
                  contentPadding: EdgeInsets.zero,
                  title: const Text('Start'),
                  trailing: Text(_fmt(start)),
                  onTap: () async {
                    final picked = await showDatePicker(
                      context: context,
                      initialDate: start,
                      firstDate: now,
                      lastDate: now.add(const Duration(days: 30)),
                    );
                    if (picked == null) return;
                    if (!context.mounted) return;
                    final time = await showTimePicker(
                      context: context,
                      initialTime: TimeOfDay.fromDateTime(start),
                    );
                    if (time == null) return;
                    setDialogState(() {
                      start = DateTime(picked.year, picked.month, picked.day,
                          time.hour, time.minute);
                      end = start.add(const Duration(hours: 2));
                    });
                  },
                ),
                ListTile(
                  contentPadding: EdgeInsets.zero,
                  title: const Text('End'),
                  trailing: Text(_fmt(end)),
                  onTap: () async {
                    final picked = await showDatePicker(
                      context: context,
                      initialDate: end,
                      firstDate: now,
                      lastDate: now.add(const Duration(days: 30)),
                    );
                    if (picked == null) return;
                    if (!context.mounted) return;
                    final time = await showTimePicker(
                      context: context,
                      initialTime: TimeOfDay.fromDateTime(end),
                    );
                    if (time == null) return;
                    setDialogState(() {
                      end = DateTime(picked.year, picked.month, picked.day,
                          time.hour, time.minute);
                    });
                  },
                ),
                if (pickedPath == null)
                  OutlinedButton.icon(
                    onPressed: () async {
                      try {
                        final picked =
                            await FilePicker.pickFile(type: FileType.image);
                        final path = picked?.path;
                        if (path == null || !context.mounted) return;
                        final manifest = await context
                            .read<MediaService>()
                            .uploadMedia(path);
                        final hash = manifest['blob_hash'] as String? ?? '';
                        if (hash.length != 64) {
                          throw Exception('Bad upload manifest');
                        }
                        setDialogState(() {
                          pickedPath = path;
                          imageUrl = 'blob://$hash';
                        });
                      } catch (e) {
                        if (context.mounted) {
                          ScaffoldMessenger.of(context).showSnackBar(
                            SnackBar(content: SelectableText('Photo failed: $e')),
                          );
                        }
                      }
                    },
                    icon: const Icon(Icons.image_outlined),
                    label: const Text('Add photo'),
                  )
                else
                  Stack(
                    children: [
                      ClipRRect(
                        borderRadius: BorderRadius.circular(8),
                        child: Image.file(
                          File(pickedPath!),
                          height: 120,
                          fit: BoxFit.cover,
                        ),
                      ),
                      Positioned(
                        top: 0,
                        right: 0,
                        child: IconButton(
                          icon: const Icon(Icons.close, size: 18),
                          onPressed: () => setDialogState(() {
                            pickedPath = null;
                            imageUrl = '';
                          }),
                        ),
                      ),
                    ],
                  ),
              ],
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context, false),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(context, true),
              child: const Text('Create'),
            ),
          ],
        ),
      ),
    );

    if (ok == true) {
      if (!mounted) return;
      try {
        final session = context.read<SessionService>();
        final pubkey = session.activePubkey;
        if (pubkey == null) throw Exception('Sign in to create events');
        await context.read<EventsService>().create(
              pubkey,
              title.text.trim(),
              desc.text.trim(),
              loc.text.trim(),
              0,
              0,
              start.millisecondsSinceEpoch ~/ 1000,
              end.millisecondsSinceEpoch ~/ 1000,
              imageUrl,
            );
        await _load();
      } catch (e) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Create failed: $e')),
          );
        }
      }
    }
  }

  String _fmt(DateTime t) =>
      '${t.month}/${t.day} ${t.hour}:${t.minute.toString().padLeft(2, '0')}';

  String _audienceLabel(_AudienceMode m) {
    if (m == _AudienceMode.all) return 'All';
    if (m == _AudienceMode.friends) return 'Friends';
    if (m == _AudienceMode.fof) return 'Friends of friends';
    return 'Mine';
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Events'),
        actions: [
          PopupMenuButton<_AudienceMode>(
            tooltip: 'Find events',
            onSelected: (m) {
              setState(() => _audienceMode = m);
              _load();
            },
            itemBuilder: (context) => [
              for (final m in _AudienceMode.values)
                PopupMenuItem(
                  value: m,
                  child: Row(
                    children: [
                      if (_audienceMode == m) const Icon(Icons.check, size: 16),
                      const SizedBox(width: 8),
                      Flexible(
                        child: Text(
                          _audienceLabel(m),
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                        ),
                      ),
                    ],
                  ),
                ),
            ],
            child: const Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(Icons.explore_outlined, size: 18),
                SizedBox(width: 4),
                Text('Find events'),
              ],
            ),
          ),
        ],
      ),
      floatingActionButton: FloatingActionButton(
        onPressed: _createDialog,
        tooltip: 'Create event',
        child: const Icon(Icons.add),
      ),
      bottomNavigationBar: _audienceMode == _AudienceMode.mine
          ? null
          : Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: Row(
                children: [
                  const Icon(Icons.radar, size: 18),
                  Expanded(
                    child: SizedBox(
                      height: 48,
                      child: Slider(
                        min: 0,
                        max: 500,
                        divisions: 10,
                        value: _radiusKm,
                        label: _radiusKm == 0
                            ? 'Everywhere'
                            : '${_radiusKm.round()} km',
                        onChanged: (v) {
                          _radiusKm = v;
                          setState(() {});
                          _load();
                        },
                      ),
                    ),
                  ),
                  Text(
                    _radiusKm == 0 ? 'Anywhere' : '${_radiusKm.round()} km',
                    style: const TextStyle(fontSize: 11),
                  ),
                ],
              ),
            ),
      body: _loading
          ? const Center(child: CircularProgressIndicator())
          : Consumer<EventsService>(
              builder: (context, api, _) {
                final visible = _visibleEvents(api.events);
                return Column(
                  children: [
                    Padding(
                      padding: const EdgeInsets.symmetric(vertical: 8),
                      child: SegmentedButton<String>(
                        segments: const [
                          ButtonSegment(
                            value: 'list',
                            label: Text('List'),
                            icon: Icon(Icons.view_list_outlined),
                          ),
                          ButtonSegment(
                            value: 'calendar',
                            label: Text('Calendar'),
                            icon: Icon(Icons.calendar_month_outlined),
                          ),
                        ],
                        selected: {_viewMode},
                        onSelectionChanged: (selection) =>
                            setState(() => _viewMode = selection.first),
                      ),
                    ),
                    Expanded(
                      child: _viewMode == 'calendar'
                          ? _buildCalendar(visible)
                          : _buildList(_sortedEvents(visible, api.scores)),
                    ),
                  ],
                );
              },
            ),
    );
  }

  Widget _buildList(List<SoshalEvent> events) {
    if (events.isEmpty) {
      return const EmptyState(
        icon: Icons.event_busy_outlined,
        title: 'No events yet',
      );
    }
    return RefreshIndicator(
      onRefresh: _load,
      child: ListView.builder(
        itemExtent: 80.0,
        itemCount: events.length,
        itemBuilder: (context, index) {
          final e = events[index];
          return ListTile(
            leading: e.image.isNotEmpty
                ? CircleAvatar(
                    radius: 24,
                    child: ClipOval(
                      child: SizedBox(
                        width: 48,
                        height: 48,
                        child: _EventImage(url: e.image),
                      ),
                    ),
                  )
                : const CircleAvatar(child: Icon(Icons.event)),
            title: Text(e.title),
            subtitle: Text(
              '${e.location.isEmpty ? 'Remote' : e.location} · '
              '${e.startTime > 0 ? DateTime.fromMillisecondsSinceEpoch(e.startTime * 1000).toLocal().toString().substring(0, 16) : 'flexible'}'
              ' · ${e.attendees} going',
            ),
            isThreeLine: true,
            trailing: e.rsvpStatus.isNotEmpty
                ? Text(e.rsvpStatus,
                    style: const TextStyle(color: Colors.green, fontSize: 12))
                : null,
            onTap: () => context.push('/events/${e.id}'),
          );
        },
      ),
    );
  }

  DateTime? _eventDay(SoshalEvent e) {
    final ts = e.startTime > 0 ? e.startTime : e.createdAt;
    if (ts <= 0) return null;
    final dt = DateTime.fromMillisecondsSinceEpoch(ts * 1000).toLocal();
    return DateTime(dt.year, dt.month, dt.day);
  }

  Widget _buildCalendar(List<SoshalEvent> events) {
    final now = DateTime.now();
    final month = DateTime(now.year, now.month + _monthOffset);
    final today = DateTime(now.year, now.month, now.day);
    final dayEvents = <DateTime, List<SoshalEvent>>{};
    for (final e in events) {
      final day = _eventDay(e);
      if (day != null) {
        (dayEvents[day] ??= []).add(e);
      }
    }
    final leadingBlanks = month.weekday % 7;
    final daysInMonth = DateTime(month.year, month.month + 1, 0).day;
    final cells = <DateTime?>[
      for (var i = 0; i < leadingBlanks; i++) null,
      for (var d = 1; d <= daysInMonth; d++)
        DateTime(month.year, month.month, d),
    ];
    while (cells.length % 7 != 0) {
      cells.add(null);
    }
    final day = _selectedDay;
    return SingleChildScrollView(
      padding: const EdgeInsets.all(12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              IconButton(
                icon: const Icon(Icons.chevron_left),
                onPressed: () => setState(() => _monthOffset--),
              ),
              Expanded(
                child: Center(
                  child: Text(
                    '${_monthNames[month.month - 1]} ${month.year}',
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                ),
              ),
              IconButton(
                icon: const Icon(Icons.chevron_right),
                onPressed: () => setState(() => _monthOffset++),
              ),
            ],
          ),
          Row(
            children: [
              for (final w in _weekdays)
                Expanded(
                  child: Center(
                    child: Text(
                      w,
                      style: Theme.of(context).textTheme.bodySmall?.copyWith(
                          color: Theme.of(context).colorScheme.outline),
                    ),
                  ),
                ),
            ],
          ),
          const SizedBox(height: 4),
          GridView.count(
            crossAxisCount: 7,
            shrinkWrap: true,
            physics: const NeverScrollableScrollPhysics(),
            children: [
              for (final c in cells)
                if (c == null)
                  const SizedBox.shrink()
                else
                  _dayCell(c, today, dayEvents[c] ?? const []),
            ],
          ),
          if (day != null) ...[
            const SizedBox(height: 8),
            const Divider(),
            Text(
              _fmtFullDate(day),
              style: Theme.of(context).textTheme.titleMedium,
            ),
            ..._selectedDayPanel(events, day),
          ],
        ],
      ),
    );
  }

  Widget _dayCell(DateTime day, DateTime today, List<SoshalEvent> dayEvents) {
    final isSelected = _selectedDay == day;
    final isToday = day == today;
    final colorScheme = Theme.of(context).colorScheme;
    final tiles = dayEvents.take(4).toList();
    return Padding(
      padding: const EdgeInsets.all(2),
      child: InkWell(
        onTap: () => setState(() => _selectedDay = day),
        borderRadius: BorderRadius.circular(6),
        child: Container(
          decoration: BoxDecoration(
            color: isSelected ? colorScheme.primaryContainer : null,
            borderRadius: BorderRadius.circular(6),
            border: isToday
                ? Border.all(color: colorScheme.primary, width: 1.5)
                : null,
          ),
          child: Column(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Text('${day.day}'),
              if (tiles.isNotEmpty)
                Row(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    for (final e in tiles)
                      Padding(
                        padding: const EdgeInsets.symmetric(horizontal: 1),
                        child: ClipRRect(
                          borderRadius: BorderRadius.circular(3),
                          child: SizedBox(
                            width: 14,
                            height: 14,
                            child: _EventImage(
                                url: e.image, height: 14, iconSize: 12),
                          ),
                        ),
                      ),
                    if (dayEvents.length > 4)
                      Text(
                        '+${dayEvents.length - 4}',
                        style: TextStyle(
                          fontSize: 8,
                          color: colorScheme.primary,
                        ),
                      ),
                  ],
                ),
            ],
          ),
        ),
      ),
    );
  }

  List<Widget> _selectedDayPanel(List<SoshalEvent> events, DateTime day) {
    final dayEvents = events.where((e) => _eventDay(e) == day).toList();
    if (dayEvents.isEmpty) {
      return const [
        Padding(
          padding: EdgeInsets.symmetric(vertical: 8),
          child: Text('No events this day.'),
        ),
      ];
    }
    return [
      for (final e in dayEvents)
        ListTile(
          dense: true,
          contentPadding: EdgeInsets.zero,
          leading: e.image.isNotEmpty
              ? CircleAvatar(
                  radius: 16,
                  child: ClipOval(
                    child: SizedBox(
                      width: 32,
                      height: 32,
                      child: _EventImage(url: e.image),
                    ),
                  ),
                )
              : const CircleAvatar(child: Icon(Icons.event, size: 18)),
          title: Text(e.title),
          subtitle: Text(_timeRange(e)),
          onTap: () => context.push('/events/${e.id}'),
        ),
    ];
  }

  String _timeRange(SoshalEvent e) {
    if (e.startTime <= 0) return 'Unscheduled';
    final s = DateTime.fromMillisecondsSinceEpoch(e.startTime * 1000).toLocal();
    final line = '${_monthShort[s.month - 1]} ${s.day} · ${formatClock12h(s)}';
    if (e.endTime <= 0) return line;
    final en = DateTime.fromMillisecondsSinceEpoch(e.endTime * 1000).toLocal();
    final endClock = en.day == s.day
        ? formatClock12h(en)
        : '${_monthShort[en.month - 1]} ${en.day} · ${formatClock12h(en)}';
    return '$line – $endClock';
  }

  String _fmtFullDate(DateTime d) =>
      '${_weekdaysFull[d.weekday % 7]}, ${_monthShort[d.month - 1]} '
      '${d.day}, ${d.year}';
}

/// Event detail: RSVP, check-in and attendees.
class EventDetailScreen extends StatefulWidget {
  /// Event detail screen.
  const EventDetailScreen({super.key, required this.eventId});

  final String eventId;

  @override
  State<EventDetailScreen> createState() => _EventDetailScreenState();
}

class _EventDetailScreenState extends State<EventDetailScreen> {
  static const _monthShort = [
    'Jan',
    'Feb',
    'Mar',
    'Apr',
    'May',
    'Jun',
    'Jul',
    'Aug',
    'Sep',
    'Oct',
    'Nov',
    'Dec',
  ];
  bool _loading = true;
  String? _attendees;
  List<EventReminder> _eventReminders = [];

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      final api = context.read<EventsService>();
      await api.getEvent(widget.eventId);
      try {
        await api.fetchReminders();
        _eventReminders = api.remindersForEvent(widget.eventId);
      } catch (_) {
        _eventReminders = [];
      }
      try {
        final list = await api.getAttendees(widget.eventId);
        _attendees = list.isEmpty ? null : '${list.length} attending';
      } catch (_) {
        _attendees = null;
      }
    } catch (e) {
      debugPrint('event detail: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  Future<void> _refreshReminders() async {
    try {
      final api = context.read<EventsService>();
      await api.fetchReminders();
      if (mounted) {
        setState(() => _eventReminders = api.remindersForEvent(widget.eventId));
      }
    } catch (e) {
      debugPrint('reminders: $e');
    }
  }

  Future<void> _addReminderDialog() async {
    final e = context.read<EventsService>().detail;
    if (e == null) return;
    final controller = TextEditingController(text: e.title);
    var minutes = 10;
    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, setDialogState) => AlertDialog(
          title: const Text('Add reminder'),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              TextField(
                controller: controller,
                decoration: const InputDecoration(labelText: 'Title'),
              ),
              const SizedBox(height: 8),
              Wrap(
                spacing: 8,
                children: [
                  for (final m in const [0, 10, 30, 60, 1440])
                    ChoiceChip(
                      label: Text(_minutesLabel(m)),
                      selected: minutes == m,
                      onSelected: (_) => setDialogState(() => minutes = m),
                    ),
                ],
              ),
            ],
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context, false),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(context, true),
              child: const Text('Save'),
            ),
          ],
        ),
      ),
    );
    if (ok == true && mounted) {
      try {
        await context.read<EventsService>().upsertReminder(
              reminderId: '',
              eventId: widget.eventId,
              title: controller.text.trim(),
              startTime: e.startTime,
              minutesBefore: minutes,
            );
        await _refreshReminders();
      } catch (err) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Reminder failed: $err')),
          );
        }
      }
    }
  }

  Future<void> _deleteReminder(String reminderId) async {
    try {
      await context.read<EventsService>().deleteReminder(reminderId);
      await _refreshReminders();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Delete failed: $e')),
        );
      }
    }
  }

  String _minutesLabel(int m) {
    if (m == 0) return 'At start';
    if (m >= 60) return '${m ~/ 60} hr';
    return '$m min';
  }

  String _fireTime(EventReminder r) {
    final d = r.fireAt;
    return '${_monthShort[d.month - 1]} ${d.day} · ${formatClock12h(d)}';
  }

  void _showAttendees() {
    showDialog<void>(
      context: context,
      builder: (context) => _AttendeeModal(eventId: widget.eventId),
    );
  }

  Future<void> _rsvp(String status) async {
    try {
      final session = context.read<SessionService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) throw Exception('Sign in to RSVP');
      await context.read<EventsService>().rsvp(widget.eventId, pubkey, status);
      await _load();
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText(status)),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('RSVP failed: $e')));
      }
    }
  }

  Future<void> _checkIn() async {
    try {
      final session = context.read<SessionService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) throw Exception('Sign in to check in');
      final location = await PermissionsService.currentPosition();
      if (!location.ok) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(SnackBar(
              content: SelectableText(
                  'Check-in needs location: ${location.error}')));
        }
        return;
      }
      if (!mounted) return;
      await context.read<EventsService>().checkIn(
          widget.eventId, pubkey, location.latitude!, location.longitude!);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: SelectableText('Checked in!')),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Check-in failed: $e')));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Event')),
      body: _loading
          ? const Center(child: CircularProgressIndicator())
          : Consumer<EventsService>(
              builder: (context, api, _) {
                final e = api.detail;
                if (e == null) {
                  return const Center(child: Text('Event not found'));
                }
                return ListView(
                  padding: const EdgeInsets.all(16),
                  children: [
                    if (e.image.isNotEmpty)
                      ClipRRect(
                        borderRadius: BorderRadius.circular(12),
                        child: SizedBox(
                          height: 200,
                          child: _EventImage(url: e.image, height: 200),
                        ),
                      ),
                    const SizedBox(height: 12),
                    Text(e.title,
                        style: Theme.of(context).textTheme.headlineSmall),
                    const SizedBox(height: 8),
                    if (e.description.isNotEmpty) Text(e.description),
                    const SizedBox(height: 16),
                    ListTile(
                      contentPadding: EdgeInsets.zero,
                      leading: const Icon(Icons.place_outlined),
                      title: Text(
                          e.location.isEmpty ? 'Location TBD' : e.location),
                    ),
                    ListTile(
                      contentPadding: EdgeInsets.zero,
                      leading: const Icon(Icons.schedule),
                      title: Text(
                        e.startTime > 0
                            ? DateTime.fromMillisecondsSinceEpoch(
                                    e.startTime * 1000)
                                .toLocal()
                                .toString()
                            : 'Flexible time',
                      ),
                    ),
                    ListTile(
                      contentPadding: EdgeInsets.zero,
                      leading: const Icon(Icons.group_outlined),
                      title: Text(
                          '${e.attendees} attending${_attendees == null ? '' : ' · $_attendees'}'),
                      trailing: const Icon(Icons.chevron_right),
                      onTap: _showAttendees,
                    ),
                    const Divider(),
                    Wrap(
                      spacing: 8,
                      children: [
                        FilledButton.icon(
                          onPressed: () => _rsvp('accepted'),
                          icon: const Icon(Icons.check),
                          label: const Text('Going'),
                        ),
                        OutlinedButton.icon(
                          onPressed: () => _rsvp('declined'),
                          icon: const Icon(Icons.close),
                          label: const Text('Not going'),
                        ),
                        OutlinedButton.icon(
                          onPressed: () => _rsvp('pending'),
                          icon: const Icon(Icons.help_outline),
                          label: const Text('Maybe'),
                        ),
                        OutlinedButton.icon(
                          onPressed: _checkIn,
                          icon: const Icon(Icons.location_searching),
                          label: const Text('Check in'),
                        ),
                      ],
                    ),
                    const Divider(),
                    Row(
                      mainAxisAlignment: MainAxisAlignment.spaceBetween,
                      children: [
                        Text(
                          'Reminders',
                          style: Theme.of(context).textTheme.titleMedium,
                        ),
                        TextButton.icon(
                          onPressed: _addReminderDialog,
                          icon: const Icon(Icons.alarm_add, size: 18),
                          label: const Text('Add'),
                        ),
                      ],
                    ),
                    if (_eventReminders.isEmpty)
                      const Padding(
                        padding: EdgeInsets.symmetric(vertical: 8),
                        child: Text(
                          'No reminders yet — add one to get a heads-up before the event.',
                        ),
                      )
                    else
                      for (final r in _eventReminders)
                        ListTile(
                          contentPadding: EdgeInsets.zero,
                          dense: true,
                          leading: const Icon(Icons.alarm, size: 18),
                          title: Text(
                            r.title.isEmpty ? 'Reminder' : r.title,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                          ),
                          subtitle: Text(
                              '${_minutesLabel(r.minutesBefore)} · ${_fireTime(r)}'),
                          trailing: IconButton(
                            icon: const Icon(Icons.delete_outline, size: 18),
                            tooltip: 'Delete reminder',
                            onPressed: () => _deleteReminder(r.id),
                          ),
                        ),
                    const SizedBox(height: 4),
                    Text(
                      'Reminder firing (notifications) lands with the '
                      'notification backend — saved locally for now.',
                      style: TextStyle(
                        fontSize: 11,
                        color: Theme.of(context).colorScheme.outline,
                      ),
                    ),
                  ],
                );
              },
            ),
    );
  }
}

/// Attendee list modal: short-form pubkeys, count header, loading/empty states.
class _AttendeeModal extends StatefulWidget {
  const _AttendeeModal({required this.eventId});

  final String eventId;

  @override
  State<_AttendeeModal> createState() => _AttendeeModalState();
}

class _AttendeeModalState extends State<_AttendeeModal> {
  late final Future<List<String>> _future;
  String? _eventTitle;

  @override
  void initState() {
    super.initState();
    _eventTitle = context.read<EventsService>().detail?.title;
    _future = context.read<EventsService>().getAttendees(widget.eventId);
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text('Attendees${_eventTitle == null ? '' : ' · $_eventTitle'}'),
      content: SizedBox(
        width: double.maxFinite,
        child: FutureBuilder<List<String>>(
          future: _future,
          builder: (context, snap) {
            if (snap.connectionState != ConnectionState.done) {
              return const Padding(
                padding: EdgeInsets.all(24),
                child: Center(child: CircularProgressIndicator()),
              );
            }
            if (snap.hasError) {
              return Text('Failed to load attendees: ${snap.error}');
            }
            final list = snap.data ?? const [];
            if (list.isEmpty) {
              return const Padding(
                padding: EdgeInsets.symmetric(vertical: 16),
                child: Text('No attendees yet.'),
              );
            }
            return Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  '${list.length} attending',
                  style: Theme.of(context).textTheme.bodySmall,
                ),
                const SizedBox(height: 8),
                Flexible(
                  child: ListView.builder(
                    shrinkWrap: true,
                    itemExtent: 40.0,
                    itemCount: list.length,
                    itemBuilder: (context, i) => ListTile(
                      dense: true,
                      contentPadding: EdgeInsets.zero,
                      leading: const Icon(Icons.person_outline, size: 18),
                      title: Text(
                        shortPubkey(list[i], head: 8, tail: 3, minLen: 13),
                        style: const TextStyle(fontSize: 12),
                      ),
                    ),
                  ),
                ),
              ],
            );
          },
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Close'),
        ),
      ],
    );
  }
}
