import 'package:flutter/material.dart';
import '../services/friends_service.dart';

/// Common small dropdown button component that organizes content
/// across features by: All, Friends of Friends, and Friends.
class AudienceFilterDropdown extends StatelessWidget {
  /// Currently selected audience filter.
  final AudienceFilter value;

  /// Callback when a new filter is selected.
  final ValueChanged<AudienceFilter> onChanged;

  /// Optional tooltip override.
  final String? tooltip;

  /// Whether to use an extra compact representation.
  final bool compact;

  const AudienceFilterDropdown({
    super.key,
    required this.value,
    required this.onChanged,
    this.tooltip,
    this.compact = false,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final colorScheme = theme.colorScheme;

    return PopupMenuButton<AudienceFilter>(
      tooltip: tooltip ?? 'Audience: ${value.label}',
      initialValue: value,
      onSelected: onChanged,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(12),
      ),
      position: PopupMenuPosition.under,
      itemBuilder: (context) => [
        for (final filter in AudienceFilter.values)
          PopupMenuItem<AudienceFilter>(
            value: filter,
            child: Row(
              children: [
                Icon(
                  filter.icon,
                  size: 18,
                  color: value == filter
                      ? colorScheme.primary
                      : colorScheme.onSurfaceVariant,
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: Text(
                    filter.label,
                    style: theme.textTheme.bodyMedium?.copyWith(
                      fontWeight:
                          value == filter ? FontWeight.w600 : FontWeight.normal,
                      color: value == filter
                          ? colorScheme.primary
                          : colorScheme.onSurface,
                    ),
                  ),
                ),
                if (value == filter) ...[
                  const SizedBox(width: 8),
                  Icon(Icons.check, size: 16, color: colorScheme.primary),
                ],
              ],
            ),
          ),
      ],
      child: Container(
        height: 32,
        padding: EdgeInsets.symmetric(
          horizontal: compact ? 8 : 10,
          vertical: 4,
        ),
        margin: const EdgeInsets.symmetric(horizontal: 4, vertical: 8),
        decoration: BoxDecoration(
          color: colorScheme.surfaceContainerHighest.withValues(alpha: 0.65),
          borderRadius: BorderRadius.circular(16),
          border: Border.all(
            color: colorScheme.outlineVariant.withValues(alpha: 0.5),
            width: 1,
          ),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.center,
          children: [
            Icon(
              value.icon,
              size: 15,
              color: colorScheme.onSurfaceVariant,
            ),
            const SizedBox(width: 5),
            Text(
              compact ? value.shortLabel : value.label,
              style: theme.textTheme.labelMedium?.copyWith(
                fontWeight: FontWeight.w600,
                color: colorScheme.onSurfaceVariant,
                fontSize: 12,
              ),
            ),
            const SizedBox(width: 2),
            Icon(
              Icons.arrow_drop_down,
              size: 16,
              color: colorScheme.onSurfaceVariant,
            ),
          ],
        ),
      ),
    );
  }
}
