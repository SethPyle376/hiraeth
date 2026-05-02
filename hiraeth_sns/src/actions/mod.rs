use hiraeth_store::sns::SnsStore;

pub(crate) fn registry<S>() -> AwsActionRegistry<S>
where
    S: SnsStore + Send + Sync + 'static,
{
    todo!()
}
