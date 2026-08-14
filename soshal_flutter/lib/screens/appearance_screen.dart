import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/theme_service.dart';

/// Theme customization — port of the legacy theme_customization page:
/// accent hue presets + slider, background level, custom accent, font scale,
/// font family. Persists to the `theme_options` setting.
class AppearanceScreen extends StatefulWidget {
  const AppearanceScreen({super.key});

  @override
  State<AppearanceScreen> createState() => _AppearanceScreenState();
}

class _AppearanceScreenState extends State<AppearanceScreen> {
  static const presets = <(double, String)>[
    (195.0, 'Sky'),
    (5.0, 'Coral'),
    (145.0, 'Mint'),
    (210.0, 'Ocean'),
    (265.0, 'Lavender'),
    (25.0, 'Peach'),
    (75.0, 'Lime'),
    (185.0, 'Azure'),
  ];

  static const bgLevels = <(String, String)>[
    ('light', 'Light'),
    ('dark', 'Dark'),
    ('darker', 'Darker'),
    ('deepest', 'Deepest'),
  ];

  static const fonts = <(String, String)>[
    ('default', 'System'),
    ('serif', 'Serif'),
    ('monospace', 'Monospace'),
  ];

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      context.read<ThemeService>().load();
    });
  }

  @override
  Widget build(BuildContext context) {
    final theme = context.watch<ThemeService>();
    return Scaffold(
      appBar: AppBar(title: const Text('Appearance')),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Text('Theme', style: Theme.of(context).textTheme.titleMedium),
          const SizedBox(height: 8),
          Wrap(
            spacing: 8,
            children: [
              for (final (key, label) in bgLevels)
                ChoiceChip(
                  label: Text(label),
                  selected: theme.bgLevel == key,
                  onSelected: (_) => theme.update(bgLevel: key),
                ),
            ],
          ),
          const SizedBox(height: 24),
          Text('Accent color', style: Theme.of(context).textTheme.titleMedium),
          const SizedBox(height: 8),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              for (final (hue, label) in presets)
                ChoiceChip(
                  avatar: CircleAvatar(
                    backgroundColor:
                        HSVColor.fromAHSV(1.0, hue, 0.55, 0.85).toColor(),
                    radius: 8,
                  ),
                  label: Text(label),
                  selected: (theme.hue - hue).abs() < 0.5,
                  onSelected: (_) => theme.update(hue: hue),
                ),
            ],
          ),
          const SizedBox(height: 8),
          Row(
            children: [
              Expanded(
                child: Slider(
                  value: theme.hue,
                  min: 0,
                  max: 360,
                  onChanged: (v) => theme.update(hue: v),
                ),
              ),
              Text('${theme.hue.round()}°'),
            ],
          ),
          const SizedBox(height: 24),
          Text('Custom accent', style: Theme.of(context).textTheme.titleMedium),
          const SizedBox(height: 8),
          TextField(
            decoration: const InputDecoration(
              labelText: '#RRGGBB',
              hintText: '#4f9cf9',
              border: OutlineInputBorder(),
            ),
            onSubmitted: (v) => theme.update(customAccent: v.trim()),
          ),
          const SizedBox(height: 24),
          Text('Font size scale',
              style: Theme.of(context).textTheme.titleMedium),
          Row(
            children: [
              Expanded(
                child: Slider(
                  value: theme.fontScale,
                  min: 0.8,
                  max: 1.4,
                  divisions: 6,
                  onChanged: (v) => theme.update(fontScale: v),
                ),
              ),
              Text('${(theme.fontScale * 100).round()}%'),
            ],
          ),
          const SizedBox(height: 24),
          Text('Font family', style: Theme.of(context).textTheme.titleMedium),
          const SizedBox(height: 8),
          DropdownButtonFormField<String>(
            initialValue: theme.fontFamily,
            decoration: const InputDecoration(
              border: OutlineInputBorder(),
            ),
            items: [
              for (final (key, label) in fonts)
                DropdownMenuItem(value: key, child: Text(label)),
            ],
            onChanged: (v) {
              if (v != null) theme.update(fontFamily: v);
            },
          ),
          const SizedBox(height: 32),
          FilledButton.icon(
            onPressed: () async {
              await theme.save();
              if (context.mounted) {
                ScaffoldMessenger.of(context).showSnackBar(
                  const SnackBar(content: Text('Theme saved')),
                );
                context.pop();
              }
            },
            icon: const Icon(Icons.save_outlined),
            label: const Text('Save Theme'),
          ),
          const SizedBox(height: 8),
          TextButton(
            onPressed: () {
              theme.update(
                hue: 195.0,
                bgLevel: 'light',
                customAccent: '',
                fontScale: 1.0,
                fontFamily: 'default',
              );
              theme.save();
            },
            child: const Text('Reset to default'),
          ),
        ],
      ),
    );
  }
}
