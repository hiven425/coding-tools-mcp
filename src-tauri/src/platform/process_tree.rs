use std::collections::{HashSet, VecDeque};

pub(crate) fn descendant_pids(root_pid: u32, processes: &[(u32, u32)]) -> Vec<u32> {
    let mut pending = VecDeque::from([root_pid]);
    let mut seen = HashSet::from([root_pid]);
    let mut ordered = Vec::new();

    while let Some(parent_pid) = pending.pop_front() {
        for &(pid, parent) in processes {
            if parent == parent_pid && seen.insert(pid) {
                ordered.push(pid);
                pending.push_back(pid);
            }
        }
    }

    ordered
}

#[cfg(test)]
mod tests {
    use super::descendant_pids;

    #[test]
    fn finds_descendants_when_snapshot_lists_children_before_parents() {
        let processes = [(300, 200), (400, 999), (200, 100), (301, 200)];

        assert_eq!(descendant_pids(100, &processes), vec![200, 300, 301]);
    }

    #[test]
    fn ignores_unrelated_processes_and_cycles_outside_the_root_tree() {
        let processes = [(200, 100), (300, 200), (700, 701), (701, 700)];

        assert_eq!(descendant_pids(100, &processes), vec![200, 300]);
    }
}
