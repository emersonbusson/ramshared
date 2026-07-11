#[cfg(test)]
mod tests {
    use super::{
        AdapterLuid, BudgetSnapshot, DxgBudgetProvider, GpuBudgetProvider, select_adapter,
    };

    #[test]
    fn official_uapi_layouts_and_ioctl_numbers_match_wsl_618() {
        assert_eq!(super::uapi::ENUM_ADAPTERS2_IOCTL, 0xc010_4714);
        assert_eq!(super::uapi::QUERY_VIDEO_MEMORY_INFO_IOCTL, 0xc038_470a);
        assert_eq!(super::uapi::CLOSE_ADAPTER_IOCTL, 0xc004_4715);
        assert_eq!(size_of::<super::uapi::EnumAdapters2>(), 16);
        assert_eq!(size_of::<super::uapi::AdapterInfo>(), 20);
        assert_eq!(size_of::<super::uapi::QueryVideoMemoryInfo>(), 56);
    }

    #[test]
    fn adapter_selection_rejects_ambiguity() {
        let a = AdapterLuid { low: 1, high: 2 };
        let b = AdapterLuid { low: 3, high: 4 };
        assert_eq!(select_adapter(&[a], None), Ok(a));
        assert!(select_adapter(&[], None).is_err());
        assert!(select_adapter(&[a, b], None).is_err());
        assert_eq!(select_adapter(&[a, b], Some(b)), Ok(b));
        assert!(select_adapter(&[a], Some(b)).is_err());
    }

    #[test]
    fn provider_trait_carries_host_budget_fields() {
        struct Fake;
        impl GpuBudgetProvider for Fake {
            fn snapshot(&self) -> Result<BudgetSnapshot, super::DxgError> {
                Ok(BudgetSnapshot {
                    adapter: AdapterLuid { low: 7, high: 8 },
                    budget: 100,
                    current_usage: 40,
                    current_reservation: 10,
                    available_for_reservation: 60,
                    sampled_at: std::time::Instant::now(),
                })
            }
        }
        let snap = Fake.snapshot().unwrap_or_else(|e| panic!("unexpected error: {e}"));
        assert_eq!(snap.budget, 100);
        let _type_check: Option<DxgBudgetProvider> = None;
    }
}
