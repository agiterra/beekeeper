part of '../project_tree.dart';

/// The three axes of the session filter, as a sheet (desktop
/// `ProjectSessionFilterMenu`): whose sessions — My / All / Custom with a
/// box per founder seen in this project, so the filter can never hide a
/// session with no way to reveal it again; whether closed sessions show;
/// and a last-activity window. Every change applies at once.
class _ProjectFilterSheet extends HookWidget {
  final ProjectSessionFilter filter;
  final List<String> founders;
  final String Function(String pubkey) ownerLabel;
  final String? myPubkey;
  final ValueChanged<ProjectSessionFilter> onChange;

  const _ProjectFilterSheet({
    required this.filter,
    required this.founders,
    required this.ownerLabel,
    required this.myPubkey,
    required this.onChange,
  });

  @override
  Widget build(BuildContext context) {
    final current = useState(filter);
    void apply(ProjectSessionFilter next) {
      current.value = next;
      onChange(next);
    }

    final members = current.value.members;
    final custom = members is ProjectSessionMembersCustom ? members : null;
    final selectable = <String>{
      if (myPubkey != null) myPubkey!.toLowerCase(),
      ...founders,
      ...?custom?.pubkeys,
    }.toList();
    final mode = switch (members) {
      ProjectSessionMembersMine() => 'mine',
      ProjectSessionMembersAll() => 'all',
      ProjectSessionMembersCustom() => 'custom',
    };

    void setMode(String? next) {
      switch (next) {
        case 'mine':
          apply(
            current.value.copyWith(members: ProjectSessionMemberFilter.mine),
          );
        case 'all':
          apply(
            current.value.copyWith(members: ProjectSessionMemberFilter.all),
          );
        case 'custom':
          if (custom != null) return;
          // Seed with the viewer so a fresh custom set is never empty.
          apply(
            current.value.copyWith(
              members: ProjectSessionMembersCustom([
                if (myPubkey != null) myPubkey!.toLowerCase(),
              ]),
            ),
          );
      }
    }

    void toggleMember(String pubkey, bool checked) {
      final keys = {...?custom?.pubkeys};
      if (checked) {
        keys.add(pubkey);
      } else {
        keys.remove(pubkey);
      }
      apply(
        current.value.copyWith(
          members: ProjectSessionMembersCustom(keys.toList()..sort()),
        ),
      );
    }

    return SingleChildScrollView(
      padding: const EdgeInsets.fromLTRB(Grid.xs, 0, Grid.xs, Grid.gutter),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _SheetLabel('Whose sessions'),
          RadioGroup<String>(
            groupValue: mode,
            onChanged: setMode,
            child: Column(
              children: [
                RadioListTile<String>(
                  key: const ValueKey('project-filter-mine'),
                  value: 'mine',
                  dense: true,
                  title: const Text('My sessions'),
                ),
                RadioListTile<String>(
                  key: const ValueKey('project-filter-all'),
                  value: 'all',
                  dense: true,
                  title: const Text('All sessions'),
                ),
                RadioListTile<String>(
                  key: const ValueKey('project-filter-custom'),
                  value: 'custom',
                  dense: true,
                  title: const Text('Custom'),
                ),
              ],
            ),
          ),
          if (custom != null)
            for (final pubkey in selectable)
              CheckboxListTile(
                key: ValueKey('project-filter-member-$pubkey'),
                dense: true,
                controlAffinity: ListTileControlAffinity.leading,
                value: custom.pubkeys.contains(pubkey),
                title: Text(
                  pubkey == myPubkey?.toLowerCase()
                      ? '${ownerLabel(pubkey)} (you)'
                      : ownerLabel(pubkey),
                ),
                onChanged: (checked) => toggleMember(pubkey, checked == true),
              ),
          const SizedBox(height: Grid.xxs),
          CheckboxListTile(
            key: const ValueKey('project-filter-show-closed'),
            dense: true,
            controlAffinity: ListTileControlAffinity.leading,
            value: current.value.showClosed,
            title: const Text('Show closed sessions'),
            onChanged: (checked) =>
                apply(current.value.copyWith(showClosed: checked == true)),
          ),
          const SizedBox(height: Grid.xxs),
          _SheetLabel('Last activity'),
          RadioGroup<ProjectSessionDateRange>(
            groupValue: current.value.range,
            onChanged: (range) {
              if (range != null) apply(current.value.copyWith(range: range));
            },
            child: Column(
              children: [
                for (final range in ProjectSessionDateRange.values)
                  RadioListTile<ProjectSessionDateRange>(
                    key: ValueKey('project-filter-range-${range.wire}'),
                    value: range,
                    dense: true,
                    title: Text(range.label),
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _SheetLabel extends StatelessWidget {
  final String text;

  const _SheetLabel(this.text);

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.fromLTRB(Grid.xs, Grid.xs, Grid.xs, Grid.quarter),
    child: Text(
      text,
      style: context.textTheme.labelLarge?.copyWith(
        color: context.colors.onSurfaceVariant,
      ),
    ),
  );
}
