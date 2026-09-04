import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/dating_service.dart';
import '../services/events_service.dart';
import '../services/media_service.dart';
import '../services/permissions_service.dart';
import '../services/session_service.dart';
import '../widgets/blob_image.dart';

/// My dating profile: create or edit.
class DatingProfileScreen extends StatefulWidget {
  /// My dating profile screen.
  const DatingProfileScreen({super.key});

  @override
  State<DatingProfileScreen> createState() => _DatingProfileScreenState();
}

const _genders = ['', 'male', 'female', 'non-binary', 'other'];
const kSeekingOptions = ['', 'male', 'female', 'non-binary', 'other', 'All'];
const _bodyTypes = ['', 'slim', 'athletic', 'average', 'curvy', 'muscular'];
const kSmokingOptions = ['', 'never', 'occasionally', 'regularly'];
const kDrinkingOptions = ['', 'never', 'socially', 'regularly'];
const _intents = ['', 'serious', 'casual', 'still figuring out'];
const kPoliticsOptions = [
  '',
  'prefer not to say',
  'liberal',
  'moderate',
  'conservative',
  'libertarian',
  'other',
];
const kEducationOptions = [
  '',
  'high school',
  'some college',
  'associate',
  'trade school',
  "bachelor's",
  "master's",
  'doctorate',
];
const _heights = [
  0,
  120,
  125,
  130,
  135,
  140,
  145,
  150,
  155,
  160,
  165,
  170,
  175,
  180,
  185,
  190,
  195,
  200,
  205,
  210,
  215,
  220,
  225,
  230,
];

class _DatingProfileScreenState extends State<DatingProfileScreen> {
  final _name = TextEditingController();
  final _age = TextEditingController();
  final _location = TextEditingController();
  final _bio = TextEditingController();
  final _interests = TextEditingController();
  final List<String> _imageHashes = [];
  bool _uploadingImage = false;
  final _ethnicity = TextEditingController();
  final _language = TextEditingController();
  String _gender = '';
  String _seeking = '';
  int _heightCm = 0;
  String _bodyType = '';
  String _smoking = '';
  String _drinking = '';
  String _relationshipIntent = '';
  String _politics = '';
  String _education = '';
  int _maxDistanceKm = 100;
  bool _saving = false;
  bool _hasProfile = false;

  @override
  void initState() {
    super.initState();
    _load();
    _stats = _loadStats();
  }

  @override
  void dispose() {
    _name.dispose();
    _age.dispose();
    _location.dispose();
    _bio.dispose();
    _interests.dispose();
    _ethnicity.dispose();
    _language.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    try {
      final session = context.read<SessionService>();
      final api = context.read<DatingService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) return;
      try {
        final own = await api.getOwnProfile(pubkey);
        _name.text = own.name;
        _age.text = own.age > 0 ? '${own.age}' : '';
        _location.text = own.location;
        _gender = own.gender;
        _seeking = own.seeking;
        _heightCm = own.height.round();
        _bodyType = own.bodyType;
        _smoking = own.smoking;
        _drinking = own.drinking;
        _relationshipIntent = own.relationshipIntent;
        _politics = own.politics;
        _ethnicity.text = own.ethnicity;
        _education = own.education;
        _language.text = own.language.join(', ');
        _maxDistanceKm =
            own.maxDistanceKm > 0 ? own.maxDistanceKm.round() : _maxDistanceKm;
        _bio.text = own.bio;
        _interests.text = own.interests.join(', ');
        _imageHashes
          ..clear()
          ..addAll(own.images);
        _hasProfile = true;
      } catch (_) {
        _hasProfile = false;
      }
    } catch (e) {
      debugPrint('dating me load: $e');
    }
    if (mounted) setState(() {});
  }

  Future<void> _useMyLocation() async {
    final events = context.read<EventsService>();
    try {
      final location = await PermissionsService.currentPosition();
      if (!location.ok) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(SnackBar(
              content: Text('Location unavailable: ${location.error}')));
        }
        return;
      }
      final geohash = events.encodeGeohash(
          lat: location.latitude!, lon: location.longitude!);
      if (!mounted) return;
      setState(() => _location.text = geohash);
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: Text('Geohash: $geohash '
              '(${location.latitude!.toStringAsFixed(4)}, '
              '${location.longitude!.toStringAsFixed(4)})')));
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Location failed: $e')));
      }
    }
  }

  Future<void> _save() async {
    setState(() => _saving = true);
    try {
      final session = context.read<SessionService>();
      final api = context.read<DatingService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) throw Exception('Sign in');
      final images = List<String>.from(_imageHashes);
      final interests = _interests.text
          .split(',')
          .map((e) => e.trim())
          .where((e) => e.isNotEmpty)
          .toList();
      final language = _language.text
          .split(',')
          .map((e) => e.trim())
          .where((e) => e.isNotEmpty)
          .toList();
      final attrs = (
        location: _location.text.trim(),
        gender: _gender,
        seeking: _seeking,
        heightCm: _heightCm,
        bodyType: _bodyType,
        smoking: _smoking,
        drinking: _drinking,
        relationshipIntent: _relationshipIntent,
        politics: _politics,
        ethnicity: _ethnicity.text.trim(),
        education: _education,
        language: language,
        maxDistanceKm: _maxDistanceKm,
      );
      if (_hasProfile) {
        await api.updateProfile(
          pubkey,
          _bio.text.trim(),
          images,
          interests,
          location: attrs.location,
          gender: attrs.gender,
          seeking: attrs.seeking,
          heightCm: attrs.heightCm,
          bodyType: attrs.bodyType,
          smoking: attrs.smoking,
          drinking: attrs.drinking,
          relationshipIntent: attrs.relationshipIntent,
          politics: attrs.politics,
          ethnicity: attrs.ethnicity,
          education: attrs.education,
          language: attrs.language,
          maxDistanceKm: attrs.maxDistanceKm,
        );
      } else {
        await api.createProfile(
          pubkey,
          _name.text.trim(),
          int.tryParse(_age.text.trim()) ?? 0,
          attrs.location,
          gender: attrs.gender,
          seeking: attrs.seeking,
          heightCm: attrs.heightCm,
          bodyType: attrs.bodyType,
          smoking: attrs.smoking,
          drinking: attrs.drinking,
          relationshipIntent: attrs.relationshipIntent,
          politics: attrs.politics,
          ethnicity: attrs.ethnicity,
          education: attrs.education,
          language: attrs.language,
          maxDistanceKm: attrs.maxDistanceKm,
          bio: _bio.text.trim(),
          images: images,
          interests: interests,
        );
      }
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: SelectableText('Dating profile saved')),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Save error: $e')));
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  Widget _dropdown({
    required String label,
    required String value,
    required List<String> options,
    required ValueChanged<String> onChanged,
    String? hint,
  }) {
    return DropdownButtonFormField<String>(
      initialValue: value,
      decoration: InputDecoration(labelText: label, hintText: hint ?? label),
      items: [
        for (final o in options)
          DropdownMenuItem(value: o, child: Text(o.isEmpty ? 'Not set' : o)),
      ],
      onChanged: (v) => onChanged(v ?? ''),
    );
  }

  Widget _stat(String label, String value) {
    return Column(
      children: [
        Text(value,
            style: const TextStyle(fontWeight: FontWeight.bold, fontSize: 16)),
        Text(label, style: const TextStyle(fontSize: 11)),
      ],
    );
  }

  Future<void> _delete() async {
    try {
      final session = context.read<SessionService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) return;
      await context.read<DatingService>().deleteProfile(pubkey);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: SelectableText('Dating profile deleted')),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Delete error: $e')));
      }
    }
  }

  Future<void> _addImage() async {
    try {
      final picked = await FilePicker.pickFile(type: FileType.image);
      final path = picked?.path;
      if (path == null || !mounted) return;
      setState(() => _uploadingImage = true);
      final manifest = await context.read<MediaService>().uploadMedia(path);
      final hash = manifest['blob_hash'] as String? ?? '';
      if (hash.length != 64) {
        throw Exception('Image upload failed (bad manifest)');
      }
      if (_imageHashes.length >= 9) {
        throw Exception('Max 9 photos');
      }
      if (!mounted) return;
      setState(() => _imageHashes.add('n$hash'));
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Upload error: $e')));
      }
    } finally {
      if (mounted) setState(() => _uploadingImage = false);
    }
  }

  late Future<DatingStats?> _stats;

  Future<DatingStats?> _loadStats() async {
    try {
      final session = context.read<SessionService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) return null;
      return await context.read<DatingService>().getStats(pubkey);
    } catch (_) {
      return null;
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title:
            Text(_hasProfile ? 'Edit dating profile' : 'Create dating profile'),
        actions: [
          TextButton(
            onPressed: _saving ? null : _save,
            child: const Text('Save'),
          ),
        ],
      ),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          FutureBuilder<DatingStats?>(
            future: _stats,
            builder: (context, snapshot) {
              final st = snapshot.data;
              if (st == null) return const SizedBox.shrink();
              return Card(
                child: Padding(
                  padding: const EdgeInsets.all(12),
                  child: Row(
                    mainAxisAlignment: MainAxisAlignment.spaceAround,
                    children: [
                      _stat('Matches', '${st.matches}'),
                      _stat('Likes', '${st.likesReceived}'),
                      _stat('Views', '${st.profileViews}'),
                      _stat('Photos', '${st.photoCount}'),
                    ],
                  ),
                ),
              );
            },
          ),
          const SizedBox(height: 12),
          if (_hasProfile) ...[
            ListTile(
              contentPadding: EdgeInsets.zero,
              leading: const Icon(Icons.badge_outlined),
              title: Text(_name.text.trim()),
              dense: true,
            ),
          ],
          TextField(
            controller: _name,
            enabled: !_hasProfile,
            decoration: const InputDecoration(labelText: 'Name'),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _age,
            enabled: !_hasProfile,
            keyboardType: TextInputType.number,
            decoration: const InputDecoration(labelText: 'Age'),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _location,
            decoration: InputDecoration(
              labelText: 'Location',
              hintText: 'lat,lon or geohash (required)',
              helperText: 'e.g. 51.5007,-0.1246 or a geohash string',
              suffixIcon: IconButton(
                icon: const Icon(Icons.my_location),
                tooltip: 'Locate me — calculate geohash',
                onPressed: _useMyLocation,
              ),
            ),
          ),
          const SizedBox(height: 12),
          _dropdown(
            label: 'Gender',
            value: _gender,
            options: _genders,
            onChanged: (v) => setState(() => _gender = v),
          ),
          const SizedBox(height: 12),
          _dropdown(
            label: 'Seeking',
            value: _seeking,
            options: kSeekingOptions,
            onChanged: (v) => setState(() => _seeking = v),
          ),
          const SizedBox(height: 12),
          DropdownButtonFormField<int>(
            initialValue: _heightCm,
            decoration: const InputDecoration(labelText: 'Height'),
            items: [
              for (final h in _heights)
                DropdownMenuItem(
                  value: h,
                  child: Text(h == 0 ? 'Not set' : '$h cm'),
                ),
            ],
            onChanged: (v) => setState(() => _heightCm = v ?? 0),
          ),
          const SizedBox(height: 12),
          _dropdown(
            label: 'Body type',
            value: _bodyType,
            options: _bodyTypes,
            onChanged: (v) => setState(() => _bodyType = v),
          ),
          const SizedBox(height: 12),
          _dropdown(
            label: 'Smoking',
            value: _smoking,
            options: kSmokingOptions,
            onChanged: (v) => setState(() => _smoking = v),
          ),
          const SizedBox(height: 12),
          _dropdown(
            label: 'Drinking',
            value: _drinking,
            options: kDrinkingOptions,
            onChanged: (v) => setState(() => _drinking = v),
          ),
          const SizedBox(height: 12),
          _dropdown(
            label: 'Relationship intent',
            value: _relationshipIntent,
            options: _intents,
            onChanged: (v) => setState(() => _relationshipIntent = v),
          ),
          const SizedBox(height: 12),
          _dropdown(
            label: 'Politics',
            value: _politics,
            options: kPoliticsOptions,
            onChanged: (v) => setState(() => _politics = v),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _ethnicity,
            decoration: const InputDecoration(labelText: 'Ethnicity'),
          ),
          const SizedBox(height: 12),
          _dropdown(
            label: 'Education',
            value: _education,
            options: kEducationOptions,
            onChanged: (v) => setState(() => _education = v),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _language,
            decoration: const InputDecoration(
              labelText: 'Language',
              hintText: 'comma separated: English, Spanish',
            ),
          ),
          const SizedBox(height: 12),
          ListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text('Max distance'),
            subtitle:
                Text(_maxDistanceKm <= 0 ? 'Unlimited' : '$_maxDistanceKm km'),
            trailing: SizedBox(
              width: 160,
              child: Slider(
                min: 0,
                max: 500,
                divisions: 20,
                value: _maxDistanceKm.toDouble(),
                label: _maxDistanceKm <= 0 ? 'Unlimited' : '$_maxDistanceKm km',
                onChanged: (v) => setState(() => _maxDistanceKm = v.round()),
              ),
            ),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _bio,
            maxLines: 4,
            decoration: const InputDecoration(labelText: 'Bio'),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _interests,
            decoration: const InputDecoration(
              labelText: 'Interests',
              hintText: 'comma separated: hiking, music, books',
            ),
          ),
          const SizedBox(height: 12),
          const Text('Photos'),
          const SizedBox(height: 8),
          if (_imageHashes.isNotEmpty)
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                for (var i = 0; i < _imageHashes.length; i++)
                  Stack(
                    children: [
                      ClipRRect(
                        borderRadius: BorderRadius.circular(8),
                        child: BlobImage(
                          source: _imageHashes[i],
                          width: 80,
                          height: 80,
                        ),
                      ),
                      Positioned(
                        top: 0,
                        right: 0,
                        child: InkWell(
                          onTap: () => setState(() => _imageHashes.removeAt(i)),
                          child: Container(
                            decoration: const BoxDecoration(
                              color: Colors.black54,
                              shape: BoxShape.circle,
                            ),
                            child: const Icon(Icons.close,
                                size: 16, color: Colors.white),
                          ),
                        ),
                      ),
                    ],
                  ),
              ],
            ),
          const SizedBox(height: 8),
          Row(
            children: [
              OutlinedButton.icon(
                onPressed: (_imageHashes.length >= 9 || _uploadingImage)
                    ? null
                    : _addImage,
                icon: _uploadingImage
                    ? const SizedBox(
                        width: 16,
                        height: 16,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Icon(Icons.add_photo_alternate_outlined),
                label: Text(_imageHashes.length >= 9
                    ? 'Max 9 photos'
                    : 'Add photo from device'),
              ),
              const SizedBox(width: 8),
              if (_imageHashes.isNotEmpty)
                const Expanded(
                  child: Text(
                    'Photos are stored on your device and served to '
                    'nearby peers',
                    style: TextStyle(fontSize: 11, color: Colors.grey),
                  ),
                ),
            ],
          ),
          const SizedBox(height: 24),
          _saving
              ? const Center(child: CircularProgressIndicator())
              : const SizedBox.shrink(),
          if (_hasProfile) ...[
            const SizedBox(height: 24),
            OutlinedButton.icon(
              onPressed: _delete,
              icon: const Icon(Icons.delete_outline),
              label: const Text('Delete dating profile'),
              style: OutlinedButton.styleFrom(
                foregroundColor: Colors.red,
              ),
            ),
          ],
        ],
      ),
    );
  }
}
